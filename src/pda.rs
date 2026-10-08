//! Program-derived address (PDA) seeds and derivation helpers.
//!
//! | PDA          | Seeds                                        | Owner          |
//! |--------------|----------------------------------------------|----------------|
//! | Vault        | `["vault", campaign]`                        | System program |
//! | Contribution | `["contribution", campaign, contributor]`    | This program   |
//!
//! Seeds are shared between the on-chain processor and the client-side
//! builders in [`crate::instruction`], so the two can never disagree.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};

/// Seed prefix of the per-campaign vault PDA that escrows donations.
pub const VAULT_SEED: &[u8] = b"vault";

/// Seed prefix of the per-(campaign, contributor) accounting PDA.
pub const CONTRIBUTION_SEED: &[u8] = b"contribution";

/// Finds the canonical vault address and bump for `campaign`.
pub fn find_vault_address(program_id: &Pubkey, campaign: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[VAULT_SEED, campaign.as_ref()], program_id)
}

/// Finds the canonical contribution address and bump for
/// (`campaign`, `contributor`).
pub fn find_contribution_address(
    program_id: &Pubkey,
    campaign: &Pubkey,
    contributor: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[CONTRIBUTION_SEED, campaign.as_ref(), contributor.as_ref()],
        program_id,
    )
}

/// Re-creates a contribution address from a *known* bump.
///
/// Much cheaper on-chain than [`find_contribution_address`] (one hash instead
/// of a bump search), which is why the bump is cached in
/// [`Contribution::bump`](crate::state::Contribution::bump).
///
/// # Errors
/// `ProgramError::InvalidSeeds` if the seeds/bump produce an on-curve point.
pub fn create_contribution_address(
    program_id: &Pubkey,
    campaign: &Pubkey,
    contributor: &Pubkey,
    bump: u8,
) -> Result<Pubkey, ProgramError> {
    Pubkey::create_program_address(
        &[
            CONTRIBUTION_SEED,
            campaign.as_ref(),
            contributor.as_ref(),
            &[bump],
        ],
        program_id,
    )
    .map_err(|_| ProgramError::InvalidSeeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_matches_find() {
        let program_id = crate::id();
        let campaign = Pubkey::new_unique();
        let contributor = Pubkey::new_unique();
        let (address, bump) = find_contribution_address(&program_id, &campaign, &contributor);
        assert_eq!(
            create_contribution_address(&program_id, &campaign, &contributor, bump).unwrap(),
            address
        );
    }

    /// Golden vector shared with `scripts/lib/program.ts` (`findVaultAddress`)
    /// so the TypeScript client and the program cannot drift apart.
    #[test]
    fn vault_address_golden_vector() {
        let program_id = solana_program::pubkey!("CrowdFund1111111111111111111111111111111111");
        let campaign = solana_program::pubkey!("11111111111111111111111111111112");
        assert_eq!(
            find_vault_address(&program_id, &campaign).0,
            solana_program::pubkey!("DuqA9aZiB5nHQSDZs6HQCwPtzthjNhV6UGJifeDAQLMv")
        );
    }

    #[test]
    fn vault_is_unique_per_campaign() {
        let program_id = crate::id();
        let (a, _) = find_vault_address(&program_id, &Pubkey::new_unique());
        let (b, _) = find_vault_address(&program_id, &Pubkey::new_unique());
        assert_ne!(a, b);
    }
}
