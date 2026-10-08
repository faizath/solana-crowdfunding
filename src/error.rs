//! Custom program errors.
//!
//! Every variant is surfaced to clients as `ProgramError::Custom(code)` where
//! `code` is the variant's discriminant, so the numeric values are part of the
//! program's public ABI: **append new variants at the end, never reorder**.

use solana_program::program_error::ProgramError;
use thiserror::Error;

/// Errors returned by the crowdfunding program.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[repr(u32)]
pub enum CrowdfundingError {
    /// `CreateCampaign` was called with `goal == 0`.
    #[error("Campaign goal must be greater than zero")]
    InvalidGoal = 0,
    /// `CreateCampaign` was called with a deadline that is not in the future.
    #[error("Campaign deadline must be in the future")]
    DeadlineInPast = 1,
    /// A contribution arrived at or after the campaign deadline.
    #[error("Campaign has ended; contributions are closed")]
    CampaignEnded = 2,
    /// Withdraw/refund attempted before the deadline.
    #[error("Campaign is still active; deadline has not passed")]
    CampaignNotEnded = 3,
    /// Withdraw attempted while `raised < goal`.
    #[error("Campaign did not reach its goal")]
    GoalNotReached = 4,
    /// Refund attempted while `raised >= goal`.
    #[error("Campaign reached its goal; refunds are not available")]
    GoalReached = 5,
    /// Withdraw attempted on a campaign whose funds were already claimed.
    #[error("Campaign funds have already been claimed")]
    AlreadyClaimed = 6,
    /// The signer is not allowed to perform this action.
    #[error("Signer is not authorized for this action")]
    Unauthorized = 7,
    /// The supplied vault does not match `["vault", campaign]`.
    #[error("Vault account does not match the campaign's vault PDA")]
    InvalidVault = 8,
    /// The supplied contribution account does not match
    /// `["contribution", campaign, contributor]` or holds foreign data.
    #[error("Contribution account does not match the expected PDA")]
    InvalidContributionAccount = 9,
    /// Refund attempted by an account that has nothing to refund.
    #[error("No contribution to refund")]
    NothingToRefund = 10,
    /// `Contribute` was called with `amount == 0`.
    #[error("Amount must be greater than zero")]
    ZeroAmount = 11,
    /// A checked arithmetic operation overflowed or underflowed.
    #[error("Arithmetic overflow")]
    Overflow = 12,
    /// The account being initialized already holds program state.
    #[error("Account is already initialized")]
    AlreadyInitialized = 13,
    /// Account data has the wrong size or discriminator, or failed to decode.
    #[error("Account data is malformed or of the wrong type")]
    InvalidAccountData = 14,
    /// The same account was passed for two roles that must be distinct.
    #[error("The same account was supplied for two distinct roles")]
    DuplicateAccount = 15,
}

impl From<CrowdfundingError> for ProgramError {
    fn from(e: CrowdfundingError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_custom_program_error_with_stable_code() {
        assert_eq!(
            ProgramError::from(CrowdfundingError::InvalidGoal),
            ProgramError::Custom(0)
        );
        assert_eq!(
            ProgramError::from(CrowdfundingError::DuplicateAccount),
            ProgramError::Custom(15)
        );
    }

    #[test]
    fn has_human_readable_messages() {
        assert_eq!(
            CrowdfundingError::AlreadyClaimed.to_string(),
            "Campaign funds have already been claimed"
        );
    }
}
