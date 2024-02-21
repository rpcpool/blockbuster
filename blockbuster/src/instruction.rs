use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};
use solana_transaction_status::InnerInstructions;
use std::collections::{HashSet, VecDeque};

pub type IxPair<'a> = (Pubkey, &'a CompiledInstruction);

#[derive(Debug, Clone)]
pub struct InstructionBundle<'a> {
    pub txn_id: &'a str,
    pub program: Pubkey,
    pub instruction: Option<&'a CompiledInstruction>,
    pub inner_ix: Option<Vec<IxPair<'a>>>,
    pub keys: &'a [Pubkey],
    pub slot: u64,
}

impl<'a> Default for InstructionBundle<'a> {
    fn default() -> Self {
        InstructionBundle {
            txn_id: "",
            program: Pubkey::new_from_array([0; 32]),
            instruction: None,
            inner_ix: None,
            keys: &[],
            slot: 0,
        }
    }
}

pub fn order_instructions<'a>(
    programs: &HashSet<Pubkey>,
    account_keys: &[Pubkey],
    message_instructions: &'a [CompiledInstruction],
    meta_inner_instructions: &'a [InnerInstructions],
) -> VecDeque<(IxPair<'a>, Option<Vec<IxPair<'a>>>)> {
    let mut ordered_ixs: VecDeque<(IxPair, Option<Vec<IxPair>>)> = VecDeque::new();

    // Get inner instructions.
    for (outer_instruction_index, message_instruction) in message_instructions.iter().enumerate() {
        let non_hoisted_inner_instruction = meta_inner_instructions
            .iter()
            .filter(|ix| ix.index == outer_instruction_index as u8)
            .flat_map(|ix| {
                ix.instructions
                    .iter()
                    .map(|ix| {
                        let cix = &ix.instruction;
                        (account_keys[cix.program_id_index as usize], cix)
                    })
                    .collect::<Vec<IxPair>>()
            })
            .collect::<Vec<IxPair>>();

        let hoister = non_hoisted_inner_instruction.clone();
        let hoisted = hoist_known_programs(programs, hoister);
        ordered_ixs.extend(hoisted);

        let outer_ix_program_id_index = message_instruction.program_id_index as usize;
        match account_keys.get(outer_ix_program_id_index) {
            Some(outer_program_id) => {
                if programs.contains(outer_program_id) {
                    ordered_ixs.push_back((
                        (*outer_program_id, message_instruction),
                        Some(non_hoisted_inner_instruction),
                    ));
                }
            }
            None => eprintln!("outer program id deserialization error"),
        }
    }
    ordered_ixs
}

fn hoist_known_programs<'a>(
    programs: &HashSet<Pubkey>,
    ix_pairs: Vec<IxPair<'a>>,
) -> Vec<(IxPair<'a>, Option<Vec<IxPair<'a>>>)> {
    let mut hoist = Vec::new();
    // there must be a safe and less copy way to do this, I should only need to move CI, and copy the found nodes matching predicate on 172
    for (index, (pid, ci)) in ix_pairs.iter().enumerate() {
        if programs.contains(pid) {
            let mut inner_copy = vec![];
            for new_inner_elem in ix_pairs.iter().skip(index + 1) {
                if pid != &new_inner_elem.0 {
                    inner_copy.push(*new_inner_elem);
                } else {
                    break;
                }
            }

            hoist.push(((*pid, *ci), Some(inner_copy)));
        }
    }
    hoist
}
