use crate::{
    error::BlockbusterError,
    instruction::InstructionBundle,
    program_handler::{NotUsed, ParseResult, ProgramParser},
    programs::ProgramParseResult,
};
use borsh::de::BorshDeserialize;
use log::warn;
use mpl_bubblegum::{
    get_instruction_type,
    instructions::UpdateMetadataInstructionArgs,
    types::{BubblegumEventType, MetadataArgs, UpdateArgs},
};
pub use mpl_bubblegum::{types::LeafSchema, InstructionName, LeafSchemaEvent, ID};
use solana_sdk::pubkey::Pubkey;
pub use spl_account_compression::events::{
    AccountCompressionEvent::{self, ApplicationData, ChangeLog},
    ApplicationDataEvent, ChangeLogEvent, ChangeLogEventV1,
};
use spl_noop;

#[derive(Eq, PartialEq)]
pub enum Payload {
    Unknown,
    MintV1 {
        args: MetadataArgs,
    },
    Decompress {
        args: MetadataArgs,
    },
    CancelRedeem {
        root: [u8; 32],
    },
    CreatorVerification {
        creator: Pubkey,
        verify: bool,
    },
    CollectionVerification {
        collection: Pubkey,
        verify: bool,
    },
    UpdateMetadata {
        current_metadata: MetadataArgs,
        update_args: UpdateArgs,
    },
}
//TODO add more of the parsing here to minimize program transformer code
pub struct BubblegumInstruction {
    pub instruction: InstructionName,
    pub tree_update: Option<ChangeLogEventV1>,
    pub leaf_update: Option<LeafSchemaEvent>,
    pub payload: Option<Payload>,
}

impl BubblegumInstruction {
    pub fn new(ix: InstructionName) -> Self {
        BubblegumInstruction {
            instruction: ix,
            tree_update: None,
            leaf_update: None,
            payload: None,
        }
    }
}

impl ParseResult for BubblegumInstruction {
    fn result_type(&self) -> ProgramParseResult {
        ProgramParseResult::Bubblegum(self)
    }
    fn result(&self) -> &Self
    where
        Self: Sized,
    {
        self
    }
}

pub struct BubblegumParser;

impl ProgramParser for BubblegumParser {
    fn key(&self) -> Pubkey {
        ID
    }

    fn key_match(&self, key: &Pubkey) -> bool {
        key == &ID
    }
    fn handles_account_updates(&self) -> bool {
        false
    }

    fn handles_instructions(&self) -> bool {
        true
    }
    fn handle_account(
        &self,
        _account_data: &[u8],
    ) -> Result<Box<(dyn ParseResult + 'static)>, BlockbusterError> {
        Ok(Box::new(NotUsed::new()))
    }

    fn handle_instruction(
        &self,
        bundle: &InstructionBundle,
    ) -> Result<Box<(dyn ParseResult + 'static)>, BlockbusterError> {
        let InstructionBundle {
            txn_id,
            instruction,
            inner_ix,
            keys,
            ..
        } = bundle;
        let outer_ix_data = match instruction {
            Some(cix) => cix.data.as_ref(),
            _ => return Err(BlockbusterError::DeserializationError),
        };
        let ix_type = get_instruction_type(outer_ix_data);
        let mut b_inst = BubblegumInstruction::new(ix_type);
        if let Some(ixs) = inner_ix {
            for (pid, cix) in ixs {
                if pid == &spl_noop::id() && !cix.data.is_empty() {
                    match AccountCompressionEvent::try_from_slice(&cix.data) {
                        Ok(result) => match result {
                            ChangeLog(changelog_event) => {
                                let ChangeLogEvent::V1(changelog_event) = changelog_event;
                                b_inst.tree_update = Some(changelog_event);
                            }
                            ApplicationData(app_data) => {
                                let ApplicationDataEvent::V1(app_data) = app_data;
                                let app_data = app_data.application_data;

                                let event_type_byte = if !app_data.is_empty() {
                                    &app_data[0..1]
                                } else {
                                    return Err(BlockbusterError::DeserializationError);
                                };

                                match BubblegumEventType::try_from_slice(event_type_byte)? {
                                    BubblegumEventType::Uninitialized => {
                                        return Err(BlockbusterError::MissingBubblegumEventData);
                                    }
                                    BubblegumEventType::LeafSchemaEvent => {
                                        b_inst.leaf_update =
                                            Some(LeafSchemaEvent::try_from_slice(&app_data)?);
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            warn!(
                                "Error while deserializing txn {:?} with noop data: {:?}",
                                txn_id, e
                            );
                        }
                    }
                }
            }
        }

        if outer_ix_data.len() >= 8 {
            let ix_data = &outer_ix_data[8..];
            if !ix_data.is_empty() {
                match b_inst.instruction {
                    InstructionName::MintV1 => {
                        let args: MetadataArgs = MetadataArgs::try_from_slice(ix_data)?;
                        b_inst.payload = Some(Payload::MintV1 { args });
                    }
                    InstructionName::MintToCollectionV1 => {
                        let mut args: MetadataArgs = MetadataArgs::try_from_slice(ix_data)?;
                        if let Some(ref mut col) = args.collection {
                            col.verified = true;
                        }
                        b_inst.payload = Some(Payload::MintV1 { args });
                    }
                    InstructionName::DecompressV1 => {
                        let args: MetadataArgs = MetadataArgs::try_from_slice(ix_data)?;
                        b_inst.payload = Some(Payload::Decompress { args });
                    }
                    InstructionName::CancelRedeem => {
                        let slice: [u8; 32] = ix_data
                            .try_into()
                            .map_err(|_e| BlockbusterError::InstructionParsingError)?;
                        b_inst.payload = Some(Payload::CancelRedeem { root: slice });
                    }
                    InstructionName::VerifyCreator => {
                        b_inst.payload = Some(build_creator_verification_payload(keys, true)?);
                    }
                    InstructionName::UnverifyCreator => {
                        b_inst.payload = Some(build_creator_verification_payload(keys, false)?);
                    }
                    InstructionName::VerifyCollection | InstructionName::SetAndVerifyCollection => {
                        b_inst.payload = Some(build_collection_verification_payload(keys, true)?);
                    }
                    InstructionName::UnverifyCollection => {
                        b_inst.payload = Some(build_collection_verification_payload(keys, false)?);
                    }
                    InstructionName::UpdateMetadata => {
                        let args = UpdateMetadataInstructionArgs::try_from_slice(outer_ix_data)?;
                        b_inst.payload = Some(Payload::UpdateMetadata {
                            current_metadata: args.current_metadata,
                            update_args: args.update_args,
                        });
                    }
                    _ => {}
                };
            }
        }

        Ok(Box::new(b_inst))
    }
}

// See Bubblegum documentation for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md#-verify_creator-and-unverify_creator
fn build_creator_verification_payload(
    keys: &[Pubkey],
    verify: bool,
) -> Result<Payload, BlockbusterError> {
    let creator = *keys
        .get(5)
        .ok_or(BlockbusterError::InstructionParsingError)?;
    Ok(Payload::CreatorVerification { creator, verify })
}

// See Bubblegum for offsets and positions:
// https://github.com/metaplex-foundation/mpl-bubblegum/blob/main/programs/bubblegum/README.md#-verify_collection-unverify_collection-and-set_and_verify_collection
// This uses the account.  The collection is only provided as an argument for `set_and_verify_collection`.
fn build_collection_verification_payload(
    keys: &[Pubkey],
    verify: bool,
) -> Result<Payload, BlockbusterError> {
    let collection = *keys
        .get(8)
        .ok_or(BlockbusterError::InstructionParsingError)?;
    Ok(Payload::CollectionVerification { collection, verify })
}
