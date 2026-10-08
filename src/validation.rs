//! Reusable account-validation guards.
//!
//! Native programs get no help from a framework here, so every handler runs
//! its accounts through these small, single-purpose checks. Each returns a
//! precise error so failed transactions are easy to diagnose from logs.

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, msg, program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{
    error::CrowdfundingError,
    pda::{create_contribution_address, find_contribution_address, find_vault_address},
};

/// Requires `account` to have signed the transaction.
///
/// # Errors
/// `ProgramError::MissingRequiredSignature`.
pub fn assert_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        msg!("Account {} must be a signer", account.key);
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

/// Requires `account` to be passed as writable.
///
/// # Errors
/// `ProgramError::InvalidAccountData` (the runtime would reject the write
/// later anyway; failing early gives a clearer message).
pub fn assert_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        msg!("Account {} must be writable", account.key);
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

/// Requires `account` to be owned by `owner`.
///
/// # Errors
/// `ProgramError::IncorrectProgramId`.
pub fn assert_owned_by(account: &AccountInfo, owner: &Pubkey) -> ProgramResult {
    if account.owner != owner {
        msg!("Account {} has an unexpected owner", account.key);
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

/// Requires `account` to be the System program, so CPIs cannot be redirected
/// to a malicious program impersonating it.
///
/// # Errors
/// `ProgramError::IncorrectProgramId`.
pub fn assert_system_program(account: &AccountInfo) -> ProgramResult {
    if *account.key != solana_system_interface::program::ID {
        msg!("Expected the System program, got {}", account.key);
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

/// Requires two accounts that play different roles to be different accounts.
///
/// # Errors
/// [`CrowdfundingError::DuplicateAccount`].
pub fn assert_distinct(a: &AccountInfo, b: &AccountInfo) -> ProgramResult {
    if a.key == b.key {
        return Err(CrowdfundingError::DuplicateAccount.into());
    }
    Ok(())
}

/// Requires `vault` to be the canonical vault PDA of `campaign` and returns
/// its bump for use in `invoke_signed`.
///
/// The vault must also be a System-owned account: it only ever holds
/// lamports, and System `transfer` can only debit System-owned accounts.
///
/// # Errors
/// [`CrowdfundingError::InvalidVault`].
pub fn assert_vault(
    program_id: &Pubkey,
    campaign: &AccountInfo,
    vault: &AccountInfo,
) -> Result<u8, ProgramError> {
    let (expected, bump) = find_vault_address(program_id, campaign.key);
    if *vault.key != expected || *vault.owner != solana_system_interface::program::ID {
        return Err(CrowdfundingError::InvalidVault.into());
    }
    Ok(bump)
}

/// Requires `contribution` to be the canonical contribution PDA of
/// (`campaign`, `contributor`) and returns its bump.
///
/// Used on first contribution, when no bump has been cached yet.
///
/// # Errors
/// [`CrowdfundingError::InvalidContributionAccount`].
pub fn assert_contribution_pda(
    program_id: &Pubkey,
    campaign: &AccountInfo,
    contributor: &AccountInfo,
    contribution: &AccountInfo,
) -> Result<u8, ProgramError> {
    let (expected, bump) = find_contribution_address(program_id, campaign.key, contributor.key);
    if *contribution.key != expected {
        return Err(CrowdfundingError::InvalidContributionAccount.into());
    }
    Ok(bump)
}

/// Same as [`assert_contribution_pda`] but using a cached `bump`, which costs
/// a single hash instead of a bump search.
///
/// # Errors
/// [`CrowdfundingError::InvalidContributionAccount`].
pub fn assert_contribution_pda_with_bump(
    program_id: &Pubkey,
    campaign: &AccountInfo,
    contributor: &AccountInfo,
    contribution: &AccountInfo,
    bump: u8,
) -> ProgramResult {
    let expected = create_contribution_address(program_id, campaign.key, contributor.key, bump)
        .map_err(|_| CrowdfundingError::InvalidContributionAccount)?;
    if *contribution.key != expected {
        return Err(CrowdfundingError::InvalidContributionAccount.into());
    }
    Ok(())
}
