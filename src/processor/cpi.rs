//! Thin wrappers around System-program CPIs and account closing.
//!
//! Centralizing these keeps every handler's "interactions" step a one-liner
//! and guarantees the vault signer seeds are built in exactly one place.

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    pubkey::Pubkey,
    rent::Rent,
};
use solana_system_interface::instruction as system_instruction;
use solana_sysvar::Sysvar;

use crate::{error::CrowdfundingError, pda::VAULT_SEED};

/// Allocates `new_account` with `space` bytes, rent-exempt, owned by `owner`.
///
/// `signer_seeds` must be the PDA seeds when `new_account` is a PDA, or empty
/// when it is a keypair that already signed the transaction.
///
/// Anyone can send lamports to an address before it is initialized, and
/// `system_instruction::create_account` refuses accounts that already hold
/// lamports. To make that griefing vector harmless, a pre-funded account is
/// instead topped up, allocated and assigned in three steps.
pub(crate) fn create_program_account<'info>(
    payer: &AccountInfo<'info>,
    new_account: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    space: usize,
    owner: &Pubkey,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let required = Rent::get()?.minimum_balance(space);
    let space_u64 = u64::try_from(space).map_err(|_| CrowdfundingError::Overflow)?;
    let current = new_account.lamports();

    if current == 0 {
        return invoke_signed(
            &system_instruction::create_account(
                payer.key,
                new_account.key,
                required,
                space_u64,
                owner,
            ),
            &[payer.clone(), new_account.clone(), system_program.clone()],
            signer_seeds,
        );
    }

    let top_up = required.saturating_sub(current);
    if top_up > 0 {
        transfer_lamports(payer, new_account, system_program, top_up)?;
    }
    invoke_signed(
        &system_instruction::allocate(new_account.key, space_u64),
        &[new_account.clone(), system_program.clone()],
        signer_seeds,
    )?;
    invoke_signed(
        &system_instruction::assign(new_account.key, owner),
        &[new_account.clone(), system_program.clone()],
        signer_seeds,
    )
}

/// Moves `amount` lamports from a signer-owned system account `from` to `to`.
pub(crate) fn transfer_lamports<'info>(
    from: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    amount: u64,
) -> ProgramResult {
    invoke(
        &system_instruction::transfer(from.key, to.key, amount),
        &[from.clone(), to.clone(), system_program.clone()],
    )
}

/// Moves `amount` lamports out of the campaign vault PDA, signing with the
/// vault seeds `["vault", campaign, bump]`.
///
/// The vault is a plain system account (no data), so the System program's
/// `transfer` is the correct and only way to debit it.
pub(crate) fn transfer_from_vault<'info>(
    vault: &AccountInfo<'info>,
    recipient: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    campaign: &Pubkey,
    vault_bump: u8,
    amount: u64,
) -> ProgramResult {
    invoke_signed(
        &system_instruction::transfer(vault.key, recipient.key, amount),
        &[vault.clone(), recipient.clone(), system_program.clone()],
        &[&[VAULT_SEED, campaign.as_ref(), &[vault_bump]]],
    )
}

/// Closes a program-owned account, sending its rent lamports to `destination`.
///
/// Data is zeroed, shrunk to 0 bytes and ownership handed back to the System
/// program, so the account cannot be "revived" with stale state later in the
/// same transaction (the classic closed-account vulnerability).
pub(crate) fn close_program_account(
    account: &AccountInfo,
    destination: &AccountInfo,
) -> ProgramResult {
    let lamports = account.lamports();
    let new_destination_balance = destination
        .lamports()
        .checked_add(lamports)
        .ok_or(CrowdfundingError::Overflow)?;
    **destination.try_borrow_mut_lamports()? = new_destination_balance;
    **account.try_borrow_mut_lamports()? = 0;

    account.try_borrow_mut_data()?.fill(0);
    account.resize(0)?;
    account.assign(&solana_system_interface::program::ID);
    Ok(())
}
