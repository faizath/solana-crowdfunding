//! On-chain account layouts.
//!
//! Every program-owned account is laid out as
//!
//! ```text
//! [ 1-byte AccountType discriminator ][ Borsh-encoded struct ]
//! ```
//!
//! The discriminator is part of the *account layout*, not of the structs, so
//! [`Campaign`] stays field-for-field identical to the project brief. It is
//! checked on every load which prevents **type confusion** (passing a
//! `Contribution` where a `Campaign` is expected) and, together with the owner
//! check, **reinitialization** of live accounts.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::{error::CrowdfundingError, validation::assert_owned_by};

/// Size in bytes of the account-type discriminator prefix.
pub const DISCRIMINATOR_LEN: usize = 1;

/// Serialized sizes of the primitive field types, used to derive the account
/// sizes below without magic numbers.
const PUBKEY_LEN: usize = 32;
const U64_LEN: usize = 8;
const I64_LEN: usize = 8;
const BOOL_LEN: usize = 1;
const U8_LEN: usize = 1;

/// Discriminator stored in the first byte of every program-owned account.
///
/// `0` is deliberately left unused: a freshly allocated account is
/// zero-filled, so it can never be mistaken for initialized state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AccountType {
    /// Zeroed / never initialized.
    Uninitialized = 0,
    /// A [`Campaign`] account.
    Campaign = 1,
    /// A [`Contribution`] account.
    Contribution = 2,
}

/// Common (de)serialization logic for discriminated program accounts.
///
/// Implementors only declare their discriminator and serialized size; loading
/// and storing – including the size, owner and discriminator checks – is
/// shared so no handler can accidentally skip them.
pub trait AccountState: BorshSerialize + BorshDeserialize {
    /// Discriminator written in front of the serialized struct.
    const ACCOUNT_TYPE: AccountType;
    /// Borsh-encoded size of the struct itself (without discriminator).
    const SERIALIZED_LEN: usize;
    /// Total account data length to allocate (discriminator + struct).
    const LEN: usize = DISCRIMINATOR_LEN + Self::SERIALIZED_LEN;

    /// Decodes `data`, verifying its exact length and discriminator.
    ///
    /// # Errors
    /// [`CrowdfundingError::InvalidAccountData`] on size mismatch, wrong
    /// discriminator or Borsh decoding failure.
    fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != Self::LEN {
            return Err(CrowdfundingError::InvalidAccountData.into());
        }
        match data.split_first() {
            Some((&tag, body)) if tag == Self::ACCOUNT_TYPE as u8 => {
                Self::try_from_slice(body).map_err(|_| CrowdfundingError::InvalidAccountData.into())
            }
            _ => Err(CrowdfundingError::InvalidAccountData.into()),
        }
    }

    /// Encodes `self` (with discriminator) into `data`.
    ///
    /// # Errors
    /// [`CrowdfundingError::InvalidAccountData`] if `data` has the wrong size.
    fn pack(&self, data: &mut [u8]) -> ProgramResult {
        if data.len() != Self::LEN {
            return Err(CrowdfundingError::InvalidAccountData.into());
        }
        let (tag, mut body) = data
            .split_first_mut()
            .ok_or(CrowdfundingError::InvalidAccountData)?;
        *tag = Self::ACCOUNT_TYPE as u8;
        self.serialize(&mut body)
            .map_err(|_| CrowdfundingError::InvalidAccountData.into())
    }

    /// Loads state from `account` after checking it is owned by `program_id`.
    ///
    /// # Errors
    /// `ProgramError::IncorrectProgramId` if the owner is wrong, otherwise the
    /// errors of [`AccountState::unpack`].
    fn load(account: &AccountInfo, program_id: &Pubkey) -> Result<Self, ProgramError> {
        assert_owned_by(account, program_id)?;
        Self::unpack(&account.try_borrow_data()?)
    }

    /// Persists `self` into `account`'s data.
    ///
    /// # Side effects
    /// Overwrites the whole account data buffer.
    fn store(&self, account: &AccountInfo) -> ProgramResult {
        self.pack(&mut account.try_borrow_mut_data()?)
    }
}

/// A crowdfunding campaign. Field-for-field the struct from the project brief.
///
/// Stored in a client-generated keypair account owned by this program.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
pub struct Campaign {
    /// Who created the campaign; the only key allowed to withdraw.
    pub creator: Pubkey,
    /// Target amount in lamports.
    pub goal: u64,
    /// Lamports contributed through [`Contribute`] (net of refunds).
    ///
    /// Lamports sent to the vault by plain transfers are *not* counted, so
    /// the goal can only be met by tracked, refundable contributions.
    ///
    /// [`Contribute`]: crate::instruction::CrowdfundingInstruction::Contribute
    pub raised: u64,
    /// Unix timestamp (seconds) at which the campaign ends.
    pub deadline: i64,
    /// Whether the creator has already withdrawn the funds.
    pub claimed: bool,
}

/// Lifecycle phase of a [`Campaign`] at a given point in time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CampaignStatus {
    /// Before the deadline: accepting contributions.
    Active,
    /// Deadline passed, goal reached, funds not yet withdrawn.
    Succeeded,
    /// Deadline passed, goal reached, funds withdrawn by the creator.
    Claimed,
    /// Deadline passed without reaching the goal: donors may refund.
    Failed,
}

impl Campaign {
    /// Returns `true` while contributions are still accepted
    /// (`now < deadline`).
    pub fn is_active(&self, now: i64) -> bool {
        now < self.deadline
    }

    /// Returns `true` once the tracked contributions cover the goal.
    pub fn goal_reached(&self) -> bool {
        self.raised >= self.goal
    }

    /// Derives the lifecycle phase from the stored fields and the clock.
    pub fn status(&self, now: i64) -> CampaignStatus {
        if self.is_active(now) {
            CampaignStatus::Active
        } else if self.claimed {
            CampaignStatus::Claimed
        } else if self.goal_reached() {
            CampaignStatus::Succeeded
        } else {
            CampaignStatus::Failed
        }
    }
}

impl AccountState for Campaign {
    const ACCOUNT_TYPE: AccountType = AccountType::Campaign;
    const SERIALIZED_LEN: usize = PUBKEY_LEN + U64_LEN + U64_LEN + I64_LEN + BOOL_LEN;
}

/// How much a single donor has contributed to a single campaign.
///
/// Stored in the PDA `["contribution", campaign, contributor]`, created on the
/// donor's first contribution. Refunds read `amount` from here, which is what
/// makes "give each donor back exactly what they gave" possible.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
pub struct Contribution {
    /// Campaign this contribution belongs to.
    pub campaign: Pubkey,
    /// Donor who made the contribution and is entitled to the refund.
    pub contributor: Pubkey,
    /// Total lamports contributed by `contributor` to `campaign`.
    pub amount: u64,
    /// Canonical bump of this PDA, cached so later instructions can verify the
    /// address with the cheap `create_program_address` instead of searching.
    pub bump: u8,
}

impl AccountState for Contribution {
    const ACCOUNT_TYPE: AccountType = AccountType::Contribution;
    const SERIALIZED_LEN: usize = PUBKEY_LEN + PUBKEY_LEN + U64_LEN + U8_LEN;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_campaign() -> Campaign {
        Campaign {
            creator: Pubkey::new_unique(),
            goal: 1_000,
            raised: 250,
            deadline: 1_700_000_000,
            claimed: false,
        }
    }

    fn sample_contribution() -> Contribution {
        Contribution {
            campaign: Pubkey::new_unique(),
            contributor: Pubkey::new_unique(),
            amount: 42,
            bump: 254,
        }
    }

    #[test]
    fn len_constants_match_borsh_encoding() {
        assert_eq!(
            borsh::to_vec(&sample_campaign()).unwrap().len(),
            Campaign::SERIALIZED_LEN
        );
        assert_eq!(
            borsh::to_vec(&sample_contribution()).unwrap().len(),
            Contribution::SERIALIZED_LEN
        );
        assert_eq!(Campaign::LEN, 58);
        assert_eq!(Contribution::LEN, 74);
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let campaign = sample_campaign();
        let mut buf = vec![0u8; Campaign::LEN];
        campaign.pack(&mut buf).unwrap();
        assert_eq!(buf[0], AccountType::Campaign as u8);
        assert_eq!(Campaign::unpack(&buf).unwrap(), campaign);

        let contribution = sample_contribution();
        let mut buf = vec![0u8; Contribution::LEN];
        contribution.pack(&mut buf).unwrap();
        assert_eq!(Contribution::unpack(&buf).unwrap(), contribution);
    }

    #[test]
    fn unpack_rejects_wrong_discriminator() {
        let mut buf = vec![0u8; Campaign::LEN];
        sample_campaign().pack(&mut buf).unwrap();
        buf[0] = AccountType::Contribution as u8;
        assert_eq!(
            Campaign::unpack(&buf),
            Err(CrowdfundingError::InvalidAccountData.into())
        );
    }

    #[test]
    fn unpack_rejects_zeroed_account() {
        let buf = vec![0u8; Campaign::LEN];
        assert_eq!(
            Campaign::unpack(&buf),
            Err(CrowdfundingError::InvalidAccountData.into())
        );
    }

    #[test]
    fn unpack_and_pack_reject_wrong_length() {
        let buf = vec![AccountType::Campaign as u8; Campaign::LEN + 1];
        assert!(Campaign::unpack(&buf).is_err());
        let mut short = vec![0u8; Campaign::LEN - 1];
        assert!(sample_campaign().pack(&mut short).is_err());
    }

    #[test]
    fn status_follows_lifecycle() {
        let mut c = sample_campaign();
        assert_eq!(c.status(c.deadline - 1), CampaignStatus::Active);
        assert_eq!(c.status(c.deadline), CampaignStatus::Failed);
        c.raised = c.goal;
        assert_eq!(c.status(c.deadline), CampaignStatus::Succeeded);
        c.claimed = true;
        assert_eq!(c.status(c.deadline), CampaignStatus::Claimed);
    }
}
