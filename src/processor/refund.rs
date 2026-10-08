//! `Refund` handler.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};

use super::{cpi, load_contribution, now, ContributionAccount};
use crate::{
    error::CrowdfundingError,
    state::{AccountState, Campaign},
    validation::{assert_signer, assert_system_program, assert_vault, assert_writable},
};

/// Accounts of [`Refund`](crate::instruction::CrowdfundingInstruction::Refund).
struct RefundAccounts<'a, 'info> {
    contributor: &'a AccountInfo<'info>,
    campaign: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    contribution: &'a AccountInfo<'info>,
    system_program: &'a AccountInfo<'info>,
}

impl<'a, 'info> RefundAccounts<'a, 'info> {
    fn parse(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let iter = &mut accounts.iter();
        let ctx = Self {
            contributor: next_account_info(iter)?,
            campaign: next_account_info(iter)?,
            vault: next_account_info(iter)?,
            contribution: next_account_info(iter)?,
            system_program: next_account_info(iter)?,
        };
        assert_signer(ctx.contributor)?;
        assert_writable(ctx.contributor)?;
        assert_writable(ctx.campaign)?;
        assert_writable(ctx.vault)?;
        assert_writable(ctx.contribution)?;
        assert_system_program(ctx.system_program)?;
        Ok(ctx)
    }
}

/// Returns a donor's contribution from the vault of a failed campaign.
///
/// # Side effects
/// * Decrements `campaign.raised` by the refunded amount so that `raised`
///   always equals the sum of outstanding contributions (and the vault
///   balance minus the rent reserve).
/// * Zeroes the donor's recorded amount, transfers the contributed lamports
///   from the vault to the donor, then closes the contribution PDA and
///   returns its rent to the donor. A second refund therefore finds no record
///   and fails with `NothingToRefund`.
///
/// # Errors
/// [`CrowdfundingError::InvalidVault`], [`CrowdfundingError::CampaignNotEnded`],
/// [`CrowdfundingError::GoalReached`],
/// [`CrowdfundingError::InvalidContributionAccount`],
/// [`CrowdfundingError::NothingToRefund`], [`CrowdfundingError::Overflow`]
/// and account-validation errors.
pub(super) fn process(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let ctx = RefundAccounts::parse(accounts)?;

    let mut campaign = Campaign::load(ctx.campaign, program_id)?;
    let vault_bump = assert_vault(program_id, ctx.campaign, ctx.vault)?;
    if campaign.is_active(now()?) {
        return Err(CrowdfundingError::CampaignNotEnded.into());
    }
    if campaign.goal_reached() {
        return Err(CrowdfundingError::GoalReached.into());
    }

    let mut record =
        match load_contribution(program_id, ctx.campaign, ctx.contributor, ctx.contribution)? {
            ContributionAccount::Initialized(record) if record.amount > 0 => record,
            _ => return Err(CrowdfundingError::NothingToRefund.into()),
        };
    let amount = record.amount;

    // Effects: both the campaign total and the donor record are settled
    // before any lamports leave the vault.
    campaign.raised = campaign
        .raised
        .checked_sub(amount)
        .ok_or(CrowdfundingError::Overflow)?;
    campaign.store(ctx.campaign)?;
    record.amount = 0;
    record.store(ctx.contribution)?;

    cpi::transfer_from_vault(
        ctx.vault,
        ctx.contributor,
        ctx.system_program,
        ctx.campaign.key,
        vault_bump,
        amount,
    )?;
    // Closing moves lamports by direct mutation, which must come after the
    // CPI: the runtime rejects a CPI whose caller has unbalanced lamports.
    cpi::close_program_account(ctx.contribution, ctx.contributor)?;

    msg!("Refunded: {} lamports", amount);
    Ok(())
}
