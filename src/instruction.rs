//! Instruction set and client-side builders.
//!
//! Instruction data is the Borsh encoding of [`CrowdfundingInstruction`]: a
//! one-byte variant index followed by the little-endian arguments.
//!
//! | Variant          | Tag | Payload                      | Total bytes |
//! |------------------|-----|------------------------------|-------------|
//! | `CreateCampaign` | `0` | `goal: u64`, `deadline: i64` | 17          |
//! | `Contribute`     | `1` | `amount: u64`                | 9           |
//! | `Withdraw`       | `2` | –                            | 1           |
//! | `Refund`         | `3` | –                            | 1           |

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::pda::{find_contribution_address, find_vault_address};

/// Instructions supported by the crowdfunding program.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Eq, PartialEq)]
pub enum CrowdfundingInstruction {
    /// Creates a new campaign and pre-funds its vault with the rent reserve.
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer, writable]` Creator – pays for the campaign account and the
    ///    vault rent reserve; recorded as the only allowed withdrawer.
    /// 1. `[signer, writable]` Campaign – fresh keypair account, allocated
    ///    by this instruction with owner = this program.
    /// 2. `[writable]` Vault PDA `["vault", campaign]`.
    /// 3. `[]` System program.
    CreateCampaign {
        /// Target amount in lamports (must be > 0).
        goal: u64,
        /// Unix timestamp (seconds) when the campaign ends (must be in the
        /// future according to the on-chain `Clock`).
        deadline: i64,
    },

    /// Donates `amount` lamports into the campaign vault.
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer, writable]` Contributor – source of the lamports; pays for
    ///    the contribution account on first contribution.
    /// 1. `[writable]` Campaign.
    /// 2. `[writable]` Vault PDA `["vault", campaign]`.
    /// 3. `[writable]` Contribution PDA `["contribution", campaign,
    ///    contributor]` – created on first contribution.
    /// 4. `[]` System program.
    Contribute {
        /// Lamports to donate (must be > 0).
        amount: u64,
    },

    /// Sends every lamport in the vault to the creator of a successful
    /// campaign. Allowed once, after the deadline, if `raised >= goal`.
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer, writable]` Creator – must equal `campaign.creator`.
    /// 1. `[writable]` Campaign.
    /// 2. `[writable]` Vault PDA `["vault", campaign]`.
    /// 3. `[]` System program.
    Withdraw,

    /// Returns a donor's full contribution from the vault of a failed campaign
    /// and closes their contribution account. Allowed after the deadline if
    /// `raised < goal`.
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer, writable]` Contributor – receives the refund and the
    ///    contribution account's rent.
    /// 1. `[writable]` Campaign.
    /// 2. `[writable]` Vault PDA `["vault", campaign]`.
    /// 3. `[writable]` Contribution PDA `["contribution", campaign,
    ///    contributor]`.
    /// 4. `[]` System program.
    Refund,
}

impl CrowdfundingInstruction {
    /// Decodes instruction data.
    ///
    /// # Errors
    /// `ProgramError::InvalidInstructionData` for unknown tags, short
    /// payloads or trailing bytes.
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        Self::try_from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)
    }
}

/// Builds a [`CrowdfundingInstruction::CreateCampaign`] instruction.
///
/// `campaign` must be a fresh keypair that also signs the transaction.
pub fn create_campaign(
    program_id: &Pubkey,
    creator: &Pubkey,
    campaign: &Pubkey,
    goal: u64,
    deadline: i64,
) -> Instruction {
    let (vault, _) = find_vault_address(program_id, campaign);
    Instruction::new_with_borsh(
        *program_id,
        &CrowdfundingInstruction::CreateCampaign { goal, deadline },
        vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new(*campaign, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
    )
}

/// Builds a [`CrowdfundingInstruction::Contribute`] instruction.
pub fn contribute(
    program_id: &Pubkey,
    contributor: &Pubkey,
    campaign: &Pubkey,
    amount: u64,
) -> Instruction {
    let (vault, _) = find_vault_address(program_id, campaign);
    let (contribution, _) = find_contribution_address(program_id, campaign, contributor);
    Instruction::new_with_borsh(
        *program_id,
        &CrowdfundingInstruction::Contribute { amount },
        vec![
            AccountMeta::new(*contributor, true),
            AccountMeta::new(*campaign, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(contribution, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
    )
}

/// Builds a [`CrowdfundingInstruction::Withdraw`] instruction.
pub fn withdraw(program_id: &Pubkey, creator: &Pubkey, campaign: &Pubkey) -> Instruction {
    let (vault, _) = find_vault_address(program_id, campaign);
    Instruction::new_with_borsh(
        *program_id,
        &CrowdfundingInstruction::Withdraw,
        vec![
            AccountMeta::new(*creator, true),
            AccountMeta::new(*campaign, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
    )
}

/// Builds a [`CrowdfundingInstruction::Refund`] instruction.
pub fn refund(program_id: &Pubkey, contributor: &Pubkey, campaign: &Pubkey) -> Instruction {
    let (vault, _) = find_vault_address(program_id, campaign);
    let (contribution, _) = find_contribution_address(program_id, campaign, contributor);
    Instruction::new_with_borsh(
        *program_id,
        &CrowdfundingInstruction::Refund,
        vec![
            AccountMeta::new(*contributor, true),
            AccountMeta::new(*campaign, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(contribution, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_is_stable() {
        let ix = CrowdfundingInstruction::CreateCampaign {
            goal: 1,
            deadline: -1,
        };
        let bytes = borsh::to_vec(&ix).unwrap();
        assert_eq!(bytes.len(), 17);
        assert_eq!(bytes[0], 0);
        assert_eq!(&bytes[1..9], &1u64.to_le_bytes());
        assert_eq!(&bytes[9..17], &(-1i64).to_le_bytes());

        let bytes = borsh::to_vec(&CrowdfundingInstruction::Contribute { amount: 7 }).unwrap();
        assert_eq!(bytes, [&[1u8][..], &7u64.to_le_bytes()].concat());
        assert_eq!(
            borsh::to_vec(&CrowdfundingInstruction::Withdraw).unwrap(),
            [2]
        );
        assert_eq!(
            borsh::to_vec(&CrowdfundingInstruction::Refund).unwrap(),
            [3]
        );
    }

    #[test]
    fn unpack_roundtrip_and_rejects_garbage() {
        let ix = CrowdfundingInstruction::Contribute { amount: 99 };
        assert_eq!(
            CrowdfundingInstruction::unpack(&borsh::to_vec(&ix).unwrap()).unwrap(),
            ix
        );
        assert_eq!(
            CrowdfundingInstruction::unpack(&[9]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            CrowdfundingInstruction::unpack(&[1, 0, 0]),
            Err(ProgramError::InvalidInstructionData)
        );
        assert_eq!(
            CrowdfundingInstruction::unpack(&[2, 0]),
            Err(ProgramError::InvalidInstructionData)
        );
    }

    #[test]
    fn builders_set_signer_and_writable_flags() {
        let program_id = crate::id();
        let (creator, campaign) = (Pubkey::new_unique(), Pubkey::new_unique());
        let ix = create_campaign(&program_id, &creator, &campaign, 10, 20);
        let flags: Vec<_> = ix
            .accounts
            .iter()
            .map(|m| (m.is_signer, m.is_writable))
            .collect();
        assert_eq!(
            flags,
            [(true, true), (true, true), (false, true), (false, false)]
        );

        let ix = refund(&program_id, &creator, &campaign);
        assert_eq!(
            ix.accounts[3].pubkey,
            find_contribution_address(&program_id, &campaign, &creator).0
        );
    }
}
