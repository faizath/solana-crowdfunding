//! `Withdraw` handler.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use super::{cpi, now};
use crate::{
    error::CrowdfundingError,
    state::{AccountState, Campaign},
    validation::{assert_signer, assert_system_program, assert_vault, assert_writable},
};

/// Accounts of [`Withdraw`](crate::instruction::CrowdfundingInstruction::Withdraw).
struct WithdrawAccounts<'a, 'info> {
    creator: &'a AccountInfo<'info>,
    campaign: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    system_program: &'a AccountInfo<'info>,
}

impl<'a, 'info> WithdrawAccounts<'a, 'info> {
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
        assert_writable(ctx.campaign)?;
        assert_writable(ctx.vault)?;
        assert_system_program(ctx.system_program)?;
        Ok(ctx)
    }
}

/// Pays out a successful campaign to its creator, exactly once.
///
/// All four conditions from the brief must hold: caller is the creator,
/// `now >= deadline`, `raised >= goal` and `!claimed`.
///
/// # Side effects
/// * Sets `campaign.claimed = true` **before** moving funds
///   (checks-effects-interactions).
/// * Transfers the vault's entire balance – contributions, the rent reserve
///   and any lamports sent to it directly – to the creator. The emptied vault
///   is garbage-collected by the runtime.
///
/// # Errors
/// [`CrowdfundingError::Unauthorized`], [`CrowdfundingError::InvalidVault`],
/// [`CrowdfundingError::CampaignNotEnded`],
/// [`CrowdfundingError::GoalNotReached`], [`CrowdfundingError::AlreadyClaimed`]
/// and account-validation errors.
pub(super) fn process(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let ctx = WithdrawAccounts::parse(accounts)?;

    let mut campaign = Campaign::load(ctx.campaign, program_id)?;
    if campaign.creator != *ctx.creator.key {
        return Err(CrowdfundingError::Unauthorized.into());
    }
    let vault_bump = assert_vault(program_id, ctx.campaign, ctx.vault)?;
    if campaign.is_active(now()?) {
        return Err(CrowdfundingError::CampaignNotEnded.into());
    }
    if !campaign.goal_reached() {
        return Err(CrowdfundingError::GoalNotReached.into());
    }
    if campaign.claimed {
        return Err(CrowdfundingError::AlreadyClaimed.into());
    }

    campaign.claimed = true;
    campaign.store(ctx.campaign)?;

    let amount = ctx.vault.lamports();
    cpi::transfer_from_vault(
        ctx.vault,
        ctx.creator,
        ctx.system_program,
        ctx.campaign.key,
        vault_bump,
        amount,
    )?;

    msg!("Withdrawn: {} lamports", amount);
    Ok(())
}
