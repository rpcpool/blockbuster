use crate::{
    error::BlockbusterError,
    program_handler::{ParseResult, ProgramParser},
    programs::ProgramParseResult,
};
use mpl_candy_machine_core::CandyMachine;
use solana_sdk::{borsh0_10::try_from_slice_unchecked, pubkey::Pubkey, pubkeys};
use std::convert::TryInto;

pubkeys!(
    candy_machine_core_id,
    "CndyV3LdqHUfDLmE5naZjVN8rBZz4tqhdefbAnjHG3JR"
);

// Anchor account discriminators.
const CANDY_MACHINE_DISCRIMINATOR: [u8; 8] = [51, 173, 177, 113, 25, 241, 109, 189];

pub enum CandyMachineCoreAccountData {
    CandyMachineCore(CandyMachine),
}

impl ParseResult for CandyMachineCoreAccountData {
    fn result_type(&self) -> ProgramParseResult {
        ProgramParseResult::CandyMachineCore(self)
    }
}

pub struct CandyMachineParser;

impl ProgramParser for CandyMachineParser {
    fn key(&self) -> Pubkey {
        candy_machine_core_id()
    }

    fn key_match(&self, key: &Pubkey) -> bool {
        key == &candy_machine_core_id()
    }
    fn handles_account_updates(&self) -> bool {
        true
    }

    fn handles_instructions(&self) -> bool {
        false
    }
    fn handle_account(
        &self,
        account_data: &[u8],
    ) -> Result<Box<dyn ParseResult>, BlockbusterError> {
        let discriminator: [u8; 8] = account_data
            .get(0..8)
            .ok_or(BlockbusterError::InvalidDataLength)?
            .try_into()
            .map_err(|_error| BlockbusterError::InvalidDataLength)?;

        let account_type = match discriminator {
            CANDY_MACHINE_DISCRIMINATOR => {
                let candy_machine = try_from_slice_unchecked(&account_data[8..])?;
                CandyMachineCoreAccountData::CandyMachineCore(candy_machine)
            }
            _ => return Err(BlockbusterError::UnknownAccountDiscriminator),
        };

        Ok(Box::new(account_type))
    }
}
