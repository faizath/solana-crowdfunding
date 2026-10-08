//! Instruction processing.
//!
//! [`process_instruction`] decodes the instruction and dispatches to one
//! handler module per instruction. Every handler follows the same shape:
//!
//! 1. **Parse** the account list into a typed struct, running the structural
//!    checks (signer, writable, program ids, distinctness).
//! 2. **Check** instruction arguments and business rules (owner,
//!    discriminator, PDA derivation, clock, goal, claimed flag).
//! 3. **Effects**: write the new state to account data.
//! 4. **Interactions**: perform System-program CPIs last
//!    (checks-effects-interactions).
//! 5. **Log** the exact message required by the project brief.

mod contribute;
mod cpi;
mod create_campaign;
mod refund;
mod withdraw;

use solana_program::{
    account_info::AccountInfo, clock::Clock, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey,
};
use solana_sysvar::Sysvar;

use crate::{
    error::CrowdfundingError,
    instruction::CrowdfundingInstruction,
    state::{AccountState, Contribution},
    validation::{assert_contribution_pda, assert_contribution_pda_with_bump},
};

/// Decodes `instruction_data` and routes it to the matching handler.
///
/// # Errors
/// `ProgramError::InvalidInstructionData` for undecodable data, otherwise the
/// error of the selected handler.
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    match CrowdfundingInstruction::unpack(instruction_data)? {
        CrowdfundingInstruction::CreateCampaign { goal, deadline } => {
            create_campaign::process(program_id, accounts, goal, deadline)
        }
        CrowdfundingInstruction::Contribute { amount } => {
            contribute::process(program_id, accounts, amount)
        }
        CrowdfundingInstruction::Withdraw => withdraw::process(program_id, accounts),
        CrowdfundingInstruction::Refund => refund::process(program_id, accounts),
    }
}

/// Current cluster time from the `Clock` sysvar.
///
/// `unix_timestamp` is the stake-weighted validator time estimate, which is
/// the closest thing to wall-clock time available on-chain. It can drift from
/// real time by seconds, which is acceptable for day-scale deadlines.
fn now() -> Result<i64, ProgramError> {
    Ok(Clock::get()?.unix_timestamp)
}

/// A validated contribution PDA, either holding state or not yet created.
enum ContributionAccount {
    /// The PDA holds a [`Contribution`] belonging to this campaign and donor.
    Initialized(Contribution),
    /// The PDA is the correct address but has not been created yet (the donor
    /// never contributed, or was already refunded). Carries the canonical bump
    /// so the caller can create it without another bump search.
    Uninitialized {
        /// Canonical bump of the contribution PDA.
        bump: u8,
    },
}

/// Loads and fully validates the contribution account of
/// (`campaign`, `contributor`).
///
/// # Errors
/// [`CrowdfundingError::InvalidContributionAccount`] if the address does not
/// match the PDA or the stored keys do not match, and
/// [`CrowdfundingError::InvalidAccountData`] for foreign data.
fn load_contribution(
    program_id: &Pubkey,
    campaign: &AccountInfo,
    contributor: &AccountInfo,
    contribution: &AccountInfo,
) -> Result<ContributionAccount, ProgramError> {
    if contribution.owner != program_id {
        // The address still has to be the canonical PDA, and it must not be
        // held by some other program.
        let bump = assert_contribution_pda(program_id, campaign, contributor, contribution)?;
        if *contribution.owner != solana_system_interface::program::ID {
            return Err(CrowdfundingError::InvalidContributionAccount.into());
        }
        return Ok(ContributionAccount::Uninitialized { bump });
    }

    let record = Contribution::load(contribution, program_id)?;
    assert_contribution_pda_with_bump(
        program_id,
        campaign,
        contributor,
        contribution,
        record.bump,
    )?;
    if record.campaign != *campaign.key || record.contributor != *contributor.key {
        return Err(CrowdfundingError::InvalidContributionAccount.into());
    }
    Ok(ContributionAccount::Initialized(record))
}
