//! `Contribute` handler.

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
    pda::CONTRIBUTION_SEED,
    state::{AccountState, Campaign, Contribution},
    validation::{assert_signer, assert_system_program, assert_vault, assert_writable},
};

/// Accounts of [`Contribute`](crate::instruction::CrowdfundingInstruction::Contribute).
struct ContributeAccounts<'a, 'info> {
    contributor: &'a AccountInfo<'info>,
    campaign: &'a AccountInfo<'info>,
    vault: &'a AccountInfo<'info>,
    contribution: &'a AccountInfo<'info>,
    system_program: &'a AccountInfo<'info>,
}

impl<'a, 'info> ContributeAccounts<'a, 'info> {
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
        // Distinctness of campaign/vault/contribution follows from the owner
        // check and the PDA derivations below; the contributor cannot alias
        // any of them because PDAs cannot sign and the campaign is
        // program-owned (it could not fund a system transfer).
        Ok(ctx)
    }
}

/// Escrows `amount` lamports in the vault and records them for the donor.
///
/// # Side effects
/// * Creates the donor's contribution PDA on first contribution (rent paid
///   by the donor, refunded to them on `Refund`).
/// * Increments `campaign.raised` and `contribution.amount`.
/// * Transfers `amount` lamports from the donor to the vault.
///
/// # Errors
/// [`CrowdfundingError::ZeroAmount`], [`CrowdfundingError::CampaignEnded`],
/// [`CrowdfundingError::InvalidVault`],
/// [`CrowdfundingError::InvalidContributionAccount`],
/// [`CrowdfundingError::Overflow`] and account-validation errors.
pub(super) fn process(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
    let ctx = ContributeAccounts::parse(accounts)?;

    if amount == 0 {
        return Err(CrowdfundingError::ZeroAmount.into());
    }
    let mut campaign = Campaign::load(ctx.campaign, program_id)?;
    if !campaign.is_active(now()?) {
        return Err(CrowdfundingError::CampaignEnded.into());
    }
    assert_vault(program_id, ctx.campaign, ctx.vault)?;

    campaign.raised = campaign
        .raised
        .checked_add(amount)
        .ok_or(CrowdfundingError::Overflow)?;

    let mut record =
        match load_contribution(program_id, ctx.campaign, ctx.contributor, ctx.contribution)? {
            ContributionAccount::Initialized(record) => record,
            ContributionAccount::Uninitialized { bump } => {
                init_contribution(program_id, &ctx, bump)?
            }
        };
    record.amount = record
        .amount
        .checked_add(amount)
        .ok_or(CrowdfundingError::Overflow)?;

    campaign.store(ctx.campaign)?;
    record.store(ctx.contribution)?;

    cpi::transfer_lamports(ctx.contributor, ctx.vault, ctx.system_program, amount)?;

    msg!(
        "Contributed: {} lamports, total={}",
        amount,
        campaign.raised
    );
    Ok(())
}

/// Allocates the donor's contribution PDA and returns its zero-amount record.
fn init_contribution(
    program_id: &Pubkey,
    ctx: &ContributeAccounts,
    bump: u8,
) -> Result<Contribution, ProgramError> {
    cpi::create_program_account(
        ctx.contributor,
        ctx.contribution,
        ctx.system_program,
        Contribution::LEN,
        program_id,
        &[&[
            CONTRIBUTION_SEED,
            ctx.campaign.key.as_ref(),
            ctx.contributor.key.as_ref(),
            &[bump],
        ]],
    )?;
    Ok(Contribution {
        campaign: *ctx.campaign.key,
        contributor: *ctx.contributor.key,
        amount: 0,
        bump,
    })
}
