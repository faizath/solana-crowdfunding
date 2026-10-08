//! `CreateCampaign` handler.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
};
use solana_sysvar::Sysvar;

use super::{cpi, now};
use crate::{
    error::CrowdfundingError,
    state::{AccountState, Campaign},
    validation::{
        assert_distinct, assert_signer, assert_system_program, assert_vault, assert_writable,
    },
};

/// Accounts of [`CreateCampaign`](crate::instruction::CrowdfundingInstruction::CreateCampaign).
struct CreateCampaignAccounts<'a, 'info> {
    creator: &'a AccountInfo<'info>,
    campaign: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    system_program: &'a AccountInfo<'info>,
}

impl<'a, 'info> CreateCampaignAccounts<'a, 'info> {
    fn parse(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let iter = &mut accounts.iter();
        let ctx = Self {
            creator: next_account_info(iter)?,
            campaign: next_account_info(iter)?,
            vault: next_account_info(iter)?,
            system_program: next_account_info(iter)?,
        };
        assert_signer(ctx.creator)?;
        assert_writable(ctx.creator)?;
        // The campaign keypair must sign so nobody can initialize an address
        // they do not control.
        assert_signer(ctx.campaign)?;
        assert_writable(ctx.campaign)?;
        assert_writable(ctx.vault)?;
        assert_system_program(ctx.system_program)?;
        assert_distinct(ctx.creator, ctx.campaign)?;
        Ok(ctx)
    }
}

/// Creates a campaign account and pre-funds its vault with the rent reserve.
///
/// # Side effects
/// * Allocates the campaign account (`Campaign::LEN` bytes, rent paid by the
///   creator, owner = this program) and writes the initial state.
/// * Transfers `Rent::minimum_balance(0)` from the creator into the vault (if
///   it does not already hold that much) so that later contributions of any
///   size, and refunds that leave only the reserve behind, never violate the
///   runtime's rent-exemption rules for the vault.
///
/// # Errors
/// [`CrowdfundingError::InvalidGoal`], [`CrowdfundingError::DeadlineInPast`],
/// [`CrowdfundingError::AlreadyInitialized`], [`CrowdfundingError::InvalidVault`]
/// and account-validation errors.
pub(super) fn process(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    goal: u64,
    deadline: i64,
) -> ProgramResult {
    let ctx = CreateCampaignAccounts::parse(accounts)?;

    if goal == 0 {
        return Err(CrowdfundingError::InvalidGoal.into());
    }
    if deadline <= now()? {
        return Err(CrowdfundingError::DeadlineInPast.into());
    }
    // Reinitialization guard: an account that already belongs to this (or
    // any other) program must never be overwritten.
    if *ctx.campaign.owner != solana_system_interface::program::ID || !ctx.campaign.data_is_empty()
    {
        return Err(CrowdfundingError::AlreadyInitialized.into());
    }
    assert_vault(program_id, ctx.campaign, ctx.vault)?;

    cpi::create_program_account(
        ctx.creator,
        ctx.campaign,
        ctx.system_program,
        Campaign::LEN,
        program_id,
        &[],
    )?;
    Campaign {
        creator: *ctx.creator.key,
        goal,
        raised: 0,
        deadline,
        claimed: false,
    }
    .store(ctx.campaign)?;

    let reserve = Rent::get()?.minimum_balance(0);
    let top_up = reserve.saturating_sub(ctx.vault.lamports());
    if top_up > 0 {
        cpi::transfer_lamports(ctx.creator, ctx.vault, ctx.system_program, top_up)?;
    }

    msg!("Campaign created: goal={}, deadline={}", goal, deadline);
    Ok(())
}
