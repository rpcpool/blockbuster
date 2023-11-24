use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};
use solana_transaction_status::InnerInstructions;
use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
};

pub type IxPair<'a> = (Pubkey, &'a CompiledInstruction);

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

    // Get account keys.
    let keys = RefCell::new(account_keys.to_vec());

    // Get inner instructions.
    for (outer_instruction_index, message_instruction) in message_instructions.iter().enumerate() {
        let non_hoisted_inner_instruction = meta_inner_instructions
            .iter()
            .filter(|ix| ix.index == outer_instruction_index as u8)
            .flat_map(|ix| {
                ix.instructions
                    .iter()
                    .map(|ix| {
                        let kb = keys.borrow();
                        let cix = &ix.instruction;
                        (kb[cix.program_id_index as usize], cix)
                    })
                    .collect::<Vec<IxPair>>()
            })
            .collect::<Vec<IxPair>>();

        let hoister = non_hoisted_inner_instruction.clone();
        let hoisted = hoist_known_programs(programs, hoister);

        for h in hoisted {
            ordered_ixs.push_back(h);
        }

        {
            let kb = keys.borrow();
            let outer_ix_program_id_index = message_instruction.program_id_index as usize;
            let outer_program_id = kb.get(outer_ix_program_id_index);
            if outer_program_id.is_none() {
                eprintln!("outer program id deserialization error");
                continue;
            }
            let outer_program_id = outer_program_id.unwrap();
            if programs.contains(outer_program_id) {
                ordered_ixs.push_back((
                    (*outer_program_id, message_instruction),
                    Some(non_hoisted_inner_instruction),
                ));
            }
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
    for (index, (pid, cix)) in ix_pairs.iter().enumerate() {
        if programs.contains(pid) {
            let mut inner_copy = vec![];
            for new_inner_elem in ix_pairs.iter().skip(index + 1) {
                if pid != &new_inner_elem.0 {
                    inner_copy.push(*new_inner_elem);
                } else {
                    break;
                }
            }

            hoist.push(((*pid, *cix), Some(inner_copy)));
        }
    }
    hoist
}
