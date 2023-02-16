// Workaround since this module is only used for testing.
#![allow(dead_code)]
use blockbuster::{
    error::BlockbusterError,
    instruction::{InstructionBundle, IxPair},
};
use borsh::ser::BorshSerialize;
use flatbuffers::{FlatBufferBuilder, WIPOffset};
pub use mpl_bubblegum::id as program_id;
use mpl_bubblegum::state::leaf_schema::LeafSchemaEvent;
use plerkle_serialization::{
    root_as_account_info, root_as_compiled_instruction, AccountInfo, AccountInfoArgs,
    CompiledInstruction, CompiledInstructionArgs, CompiledInstructionBuilder, InnerInstructions,
    InnerInstructionsArgs, InnerInstructionsBuilder, Pubkey as FBPubkey, TransactionInfo,
    TransactionInfoArgs, TransactionInfoBuilder,
};
use rand::Rng;
use solana_geyser_plugin_interface::geyser_plugin_interface::ReplicaAccountInfo;
use solana_sdk::{bs58, pubkey::Pubkey, transaction::VersionedTransaction};
use solana_transaction_status::{
    option_serializer::OptionSerializer, EncodableWithMeta,
    EncodedConfirmedTransactionWithStatusMeta, UiInstruction, UiTransactionStatusMeta, EncodedTransaction,
};
use spl_account_compression::events::{
    AccountCompressionEvent, ApplicationDataEvent, ApplicationDataEventV1,
};
use std::{fs::File, collections::HashSet};
use std::io::BufReader;
use std::str::FromStr;
pub fn random_program() -> Pubkey {
    Pubkey::new_unique()
}

pub fn random_pubkey() -> Pubkey {
    random_program()
}

pub fn random_data(max: usize) -> Vec<u8> {
    let mut s = rand::thread_rng();
    let x = s.gen_range(1..max);
    let mut data: Vec<u8> = Vec::with_capacity(x);
    for i in 0..x {
        let d: u8 = s.gen_range(0..255);
        data.insert(i, d);
    }
    data
}

pub fn random_u8() -> u8 {
    let mut s = rand::thread_rng();
    s.gen()
}

pub fn random_u8_bound(min: u8, max: u8) -> u8 {
    let mut s = rand::thread_rng();
    s.gen_range(min..max)
}

pub fn random_list(size: usize, elem_max: u8) -> Vec<u8> {
    let mut s = rand::thread_rng();
    let mut data: Vec<u8> = Vec::with_capacity(size);
    for i in 0..size {
        let d: u8 = s.gen_range(0..elem_max);
        data.insert(i, d);
    }
    data
}

pub fn random_list_of<FN, T>(size: usize, fun: FN) -> Vec<T>
where
    FN: Fn(u8) -> T,
{
    let mut s = rand::thread_rng();
    let mut data: Vec<T> = Vec::with_capacity(size);
    for i in 0..size {
        data.insert(i, fun(s.gen()));
    }
    data
}

pub fn build_random_instruction<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    accounts_number_in_transaction: usize,
    number_of_accounts: usize,
) -> WIPOffset<CompiledInstruction<'a>> {
    let accounts = random_list(5, random_u8_bound(1, number_of_accounts as u8));
    let accounts = fbb.create_vector(&accounts);
    let data = random_data(10);
    let data = fbb.create_vector(&data);
    let mut s = rand::thread_rng();
    let mut builder = CompiledInstructionBuilder::new(fbb);
    builder.add_data(data);
    builder.add_program_id_index(s.gen_range(0..accounts_number_in_transaction) as u8);
    builder.add_accounts(accounts);
    builder.finish()
}

pub fn build_random_transaction(mut fbb: FlatBufferBuilder) -> FlatBufferBuilder {
    let mut s = rand::thread_rng();
    let mut outer_instructions = vec![];
    let mut inner_instructions = vec![];
    for _ in 0..s.gen_range(2..7) {
        outer_instructions.push(build_random_instruction(&mut fbb, 10, 3));

        let mut indexed_inner_instructions = vec![];
        for _ in 0..s.gen_range(2..7) {
            let ix = build_random_instruction(&mut fbb, 10, 3);
            indexed_inner_instructions.push(ix);
        }

        let indexed_inner_instructions = fbb.create_vector(&indexed_inner_instructions);
        let mut builder = InnerInstructionsBuilder::new(&mut fbb);
        builder.add_index(s.gen_range(0..7));
        builder.add_instructions(indexed_inner_instructions);
        inner_instructions.push(builder.finish());
    }

    let outer_instructions = fbb.create_vector(&outer_instructions);
    let inner_instructions = fbb.create_vector(&inner_instructions);
    let account_keys = random_list_of(10, |_| FBPubkey(random_pubkey().to_bytes()));
    let account_keys = fbb.create_vector(&account_keys);
    let mut builder = TransactionInfoBuilder::new(&mut fbb);
    let slot = s.gen();
    builder.add_outer_instructions(outer_instructions);
    builder.add_is_vote(false);
    builder.add_inner_instructions(inner_instructions);
    builder.add_account_keys(account_keys);
    builder.add_slot(slot);
    builder.add_seen_at(s.gen());
    let builder = builder.finish();
    fbb.finish_minimal(builder);
    fbb
}

pub fn get_programs(txn_info: TransactionInfo) -> Vec<Pubkey> {
    let mut outer_keys: Vec<Pubkey> = txn_info
        .outer_instructions()
        .unwrap()
        .iter()
        .map(|ix| {
            println!("{:?}", txn_info);
            Pubkey::new(
                &txn_info
                    .account_keys()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>()
                    .get(ix.program_id_index() as usize)
                    .unwrap()
                    .0,
            )
        })
        .collect();
    let mut inner = vec![];
    let inner_keys = txn_info
        .inner_instructions()
        .unwrap()
        .iter()
        .fold(&mut inner, |ix, curr| {
            for p in curr.instructions().unwrap() {
                ix.push(Pubkey::new(
                    &txn_info
                        .account_keys()
                        .unwrap()
                        .iter()
                        .collect::<Vec<_>>()
                        .get(p.program_id_index() as usize)
                        .unwrap()
                        .0,
                ))
            }
            ix
        });
    outer_keys.append(inner_keys);
    outer_keys.dedup();
    outer_keys
}

pub fn build_instruction<'a>(
    fbb: &'a mut FlatBufferBuilder<'a>,
    data: &[u8],
    account_indexes: &[u8],
) -> Result<CompiledInstruction<'a>, flatbuffers::InvalidFlatbuffer> {
    let accounts_vec = fbb.create_vector(account_indexes);
    let ix_data = fbb.create_vector(data);
    let mut builder = CompiledInstructionBuilder::new(fbb);
    builder.add_accounts(accounts_vec);
    builder.add_program_id_index(0);
    builder.add_data(ix_data);
    let offset = builder.finish();
    fbb.finish_minimal(offset);
    let data = fbb.finished_data();

    root_as_compiled_instruction(data)
}

pub fn build_account_update<'a>(
    fbb: &'a mut FlatBufferBuilder<'a>,
    account: &ReplicaAccountInfo,
    slot: u64,
    is_startup: bool,
) -> Result<AccountInfo<'a>, flatbuffers::InvalidFlatbuffer> {
    // Serialize vector data.
    let pubkey = FBPubkey::from(account.pubkey);
    let owner = FBPubkey::from(account.owner);

    // Don't serialize a zero-length data slice.
    let data = if !account.data.is_empty() {
        Some(fbb.create_vector(account.data))
    } else {
        None
    };

    // Serialize everything into Account Info table.
    let account_info = AccountInfo::create(
        fbb,
        &AccountInfoArgs {
            pubkey: Some(&pubkey),
            lamports: account.lamports,
            owner: Some(&owner),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
            data,
            write_version: account.write_version,
            slot,
            is_startup,
            seen_at: 0,
        },
    );

    // Finalize buffer
    fbb.finish(account_info, None);
    let data = fbb.finished_data();
    root_as_account_info(data)
}

pub fn build_random_account_update<'a>(
    fbb: &'a mut FlatBufferBuilder<'a>,
    data: &[u8],
) -> Result<AccountInfo<'a>, flatbuffers::InvalidFlatbuffer> {
    // Create a `ReplicaAccountInfo` to store the account update.
    // All fields except caller-specified `data` are just random values.
    let replica_account_info = ReplicaAccountInfo {
        pubkey: &random_pubkey().to_bytes()[..],
        lamports: 1,
        owner: &random_pubkey().to_bytes()[..],
        executable: false,
        rent_epoch: 1000,
        data,
        write_version: 1,
    };

    // Flatbuffer serialize the `ReplicaAccountInfo`.
    build_account_update(fbb, &replica_account_info, 0, false)
}

pub fn  serialize_transaction<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    tx: EncodedConfirmedTransactionWithStatusMeta,
) -> Result<(), BlockbusterError> {
    let meta: UiTransactionStatusMeta = tx.transaction.meta.unwrap();    
    // Get `UiTransaction` out of `EncodedTransactionWithStatusMeta`.
    let ui_transaction: VersionedTransaction = tx.transaction.transaction.decode().unwrap();
    let msg = ui_transaction.message;
    let atl_keys = msg.address_table_lookups();
    let account_keys = msg.static_account_keys();
    let sig = ui_transaction.signatures[0].to_string();

   

    let account_keys = {
        let mut account_keys_fb_vec = vec![];
        for key in account_keys.iter() {
            account_keys_fb_vec.push(FBPubkey(key.to_bytes()));
        }
        if atl_keys.is_some() {
            if let OptionSerializer::Some(ad) = meta.loaded_addresses {
                for i in ad.writable {
                    let mut output: [u8;32] = [0; 32];
                    bs58::decode(i).into(&mut output).unwrap();
                    let pubkey = FBPubkey(output);
                    account_keys_fb_vec.push(pubkey);
                }
                
                for i in ad.readonly {
                    let mut output: [u8;32] = [0; 32];
                    bs58::decode(i).into(&mut output).unwrap();
                    let pubkey = FBPubkey(output);
                    account_keys_fb_vec.push(pubkey);
                }
                
            }
        }
        if account_keys_fb_vec.len() > 0 {
            Some(builder.create_vector(&account_keys_fb_vec))
        } else {
            None
        }
    };
   
    // Serialize log messages.
    // We dont use them for now.
    let log_messages = None;

    // Serialize inner instructions.
    let inner_instructions = if let OptionSerializer::Some(inner_instructions_vec) =
        meta.inner_instructions.as_ref()
    {
        let mut overall_fb_vec = Vec::with_capacity(inner_instructions_vec.len());
        for inner_instructions in inner_instructions_vec.iter() {
            let index = inner_instructions.index;
            let mut instructions_fb_vec = Vec::with_capacity(inner_instructions.instructions.len());
            for ui_instruction in inner_instructions.instructions.iter() {
                if let UiInstruction::Compiled(ui_compiled_instruction) = ui_instruction {
                    let program_id_index = ui_compiled_instruction.program_id_index;
                    let accounts = Some(builder.create_vector(&ui_compiled_instruction.accounts));
                    let data = bs58::decode(&ui_compiled_instruction.data)
                        .into_vec()
                        .map_err(|e| BlockbusterError::IOError(e.to_string()))?;
                    let data = Some(builder.create_vector(&data));
                    instructions_fb_vec.push(CompiledInstruction::create(
                        builder,
                        &CompiledInstructionArgs {
                            program_id_index,
                            accounts,
                            data,
                        },
                    ));
                }
            }

            let instructions = Some(builder.create_vector(&instructions_fb_vec));
            overall_fb_vec.push(InnerInstructions::create(
                builder,
                &InnerInstructionsArgs {
                    index,
                    instructions,
                },
            ))
        }

        Some(builder.create_vector(&overall_fb_vec))
    } else {
        None
    };

    // Serialize outer instructions.
    let outer_instructions = &msg.instructions();
    let outer_instructions = if !outer_instructions.is_empty() {
        let mut instructions_fb_vec = Vec::with_capacity(outer_instructions.len());
        for ui_compiled_instruction in outer_instructions.iter() {
            let program_id_index = ui_compiled_instruction.program_id_index;
            let accounts = Some(builder.create_vector(&ui_compiled_instruction.accounts));
                
            let data = Some(builder.create_vector(&ui_compiled_instruction.data));
            instructions_fb_vec.push(CompiledInstruction::create(
                builder,
                &CompiledInstructionArgs {
                    program_id_index,
                    accounts,
                    data,
                },
            ));
        }
        Some(builder.create_vector(&instructions_fb_vec))
    } else {
        None
    };

    // Serialize everything into Transaction Info table.

    let sig_db = builder.create_string(&sig);
    let transaction_info = TransactionInfo::create(
        builder,
        &TransactionInfoArgs {
            is_vote: false,
            account_keys,
            log_messages,
            inner_instructions,
            outer_instructions,
            slot: tx.slot,
            seen_at: 1000000,
            slot_index: None,
            signature: Some(sig_db),
        },
    );

    // Finalize buffer and return to caller.
    builder.finish(transaction_info, None);
    Ok(())
}

pub fn build_txn_from_fixture<'a>(
    fixture_name: String,
    fbb: &'a mut FlatBufferBuilder<'a>,
) -> Result<&'a mut FlatBufferBuilder, BlockbusterError> {
    let file = File::open(format!("{}/tests/fixtures/{}.json", env!("CARGO_MANIFEST_DIR"), fixture_name)).unwrap();
    let reader = BufReader::new(file);
    let ectxn: EncodedConfirmedTransactionWithStatusMeta = serde_json::from_reader(reader).unwrap();
    serialize_transaction(fbb, ectxn)?;
    Ok(fbb)
}

pub fn build_bubblegum_bundle<'a>(
    fbb1: &'a mut FlatBufferBuilder<'a>,
    fbb2: &'a mut FlatBufferBuilder<'a>,
    fbb3: &'a mut FlatBufferBuilder<'a>,
    fbb4: &'a mut FlatBufferBuilder<'a>,
    accounts: &'a Vec<FBPubkey>,
    account_indexes: &'a Vec<u8>,
    ix_data: &'a [u8],
    lse: LeafSchemaEvent,
    cs_event: AccountCompressionEvent,
    ixb: &mut InstructionBundle<'a>,
) {
    let lse_versioned = ApplicationDataEventV1 {
        application_data: lse.try_to_vec().unwrap(),
    };
    let lse_event =
        AccountCompressionEvent::ApplicationData(ApplicationDataEvent::V1(lse_versioned));
    let outer_ix = build_instruction(fbb1, ix_data, account_indexes).unwrap();
    let lse = lse_event.try_to_vec().unwrap();
    let noop_bgum = spl_noop::instruction(lse).data;
    let ix = build_instruction(fbb2, &noop_bgum, account_indexes).unwrap();
    let noop_bgum_ix: IxPair = (FBPubkey(spl_noop::id().to_bytes()), ix);
    // The Compression Instruction here doesnt matter only the noop but we add it here to ensure we are validating that one Account compression event is happening after Bubblegum
    let ix = build_instruction(fbb3, &[0; 0], account_indexes)
        .unwrap();
    let gummy_roll_ix: IxPair = (FBPubkey(spl_account_compression::id().to_bytes()), ix);
    let cs = cs_event.try_to_vec().unwrap();
    let noop_compression = spl_noop::instruction(cs).data;
    let ix = build_instruction(fbb4, &noop_compression, account_indexes)
        .unwrap();
    let noop_compression_ix = (FBPubkey(spl_noop::id().to_bytes()), ix);

    let inner_ix = vec![noop_bgum_ix, gummy_roll_ix, noop_compression_ix];

    ixb.program = FBPubkey(program_id().to_bytes());
    ixb.inner_ix = Some(inner_ix);
    ixb.keys = accounts.as_slice();
    ixb.instruction = Some(outer_ix);
}


