//! Program entrypoint.
//!
//! Compiled out with the `no-entrypoint` feature so that the crate can be used
//! as a library (e.g. for its instruction builders) without exporting a second
//! `entrypoint` symbol.

use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

use crate::processor;

solana_program::entrypoint!(process_instruction);

/// Runtime entrypoint: delegates straight to [`processor::process_instruction`].
///
/// Keeping this function trivial means every code path – including error
/// handling – lives in the processor where it can be unit-tested.
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    processor::process_instruction(program_id, accounts, instruction_data)
}
