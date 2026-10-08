//! `CreateCampaign` behaviour.

use solana_crowdfunding::{
    error::CrowdfundingError,
    state::{AccountState, Campaign},
};
use solana_program::instruction::InstructionError;
use solana_signer::Signer;

use crate::common::*;

#[test]
fn stores_initial_state_and_funds_rent_reserve() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, sol(5), deadline).unwrap();

    let account = ctx.svm.get_account(&campaign.pubkey()).unwrap();
    assert_eq!(account.owner, ctx.program_id);
    assert_eq!(account.data.len(), Campaign::LEN);
    assert_eq!(account.lamports, ctx.rent_exempt(Campaign::LEN));

    let vault = ctx.vault_address(&campaign.pubkey());
    let vault_account = ctx.svm.get_account(&vault).unwrap();
    assert_eq!(vault_account.owner, solana_system_interface::program::ID);
    assert!(vault_account.data.is_empty());
    assert_eq!(vault_account.lamports, ctx.rent_exempt(0));
}

#[test]
fn logs_exact_message() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() + 60;
    let campaign = solana_keypair::Keypair::new();
    let result = ctx.try_create_campaign(&creator, &campaign, 12_345, deadline);
    assert_logged(
        &result,
        &format!("Campaign created: goal=12345, deadline={deadline}"),
    );
}

#[test]
fn rejects_zero_goal() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let result = ctx.create_campaign(&creator, 0, deadline).map(|(_, m)| m);
    assert_custom_error(result, CrowdfundingError::InvalidGoal);
}

#[test]
fn rejects_deadline_in_past() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() - 1;
    let result = ctx
        .create_campaign(&creator, sol(1), deadline)
        .map(|(_, m)| m);
    assert_custom_error(result, CrowdfundingError::DeadlineInPast);
}

#[test]
fn rejects_deadline_equal_to_now() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now();
    let result = ctx
        .create_campaign(&creator, sol(1), deadline)
        .map(|(_, m)| m);
    assert_custom_error(result, CrowdfundingError::DeadlineInPast);
}

#[test]
fn rejects_reinitializing_existing_campaign() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, sol(1), deadline).unwrap();
    ctx.contribute(&creator, &campaign.pubkey(), 1_000).unwrap();

    // Same campaign keypair, new parameters: must not reset `raised`.
    let result = ctx.try_create_campaign(&creator, &campaign, sol(2), deadline + DAY);
    assert_custom_error(result, CrowdfundingError::AlreadyInitialized);
    let state = ctx.campaign(&campaign.pubkey());
    assert_eq!(state.goal, sol(1));
    assert_eq!(state.raised, 1_000);
}

#[test]
fn rejects_reinitialization_by_another_creator() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let attacker = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, sol(1), deadline).unwrap();

    let result = ctx.try_create_campaign(&attacker, &campaign, 1, deadline);
    assert_custom_error(result, CrowdfundingError::AlreadyInitialized);
    assert_eq!(ctx.campaign(&campaign.pubkey()).creator, creator.pubkey());
}

#[test]
fn requires_campaign_signature() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let campaign = solana_keypair::Keypair::new();
    let mut ix = solana_crowdfunding::instruction::create_campaign(
        &ctx.program_id,
        &creator.pubkey(),
        &campaign.pubkey(),
        sol(1),
        ctx.now() + DAY,
    );
    ix.accounts[1].is_signer = false;
    let result = ctx.send(&[ix], &[&creator]);
    assert_instruction_error(result, InstructionError::MissingRequiredSignature);
}

#[test]
fn rejects_creator_as_campaign_account() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let ix = solana_crowdfunding::instruction::create_campaign(
        &ctx.program_id,
        &creator.pubkey(),
        &creator.pubkey(),
        sol(1),
        ctx.now() + DAY,
    );
    let result = ctx.send(&[ix], &[&creator]);
    assert_custom_error(result, CrowdfundingError::DuplicateAccount);
}

#[test]
fn succeeds_when_vault_was_prefunded() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let campaign = solana_keypair::Keypair::new();
    let vault = ctx.vault_address(&campaign.pubkey());
    let gift = 5 * ctx.rent_exempt(0);
    ctx.svm.airdrop(&vault, gift).unwrap();

    let creator_before = ctx.balance(&creator.pubkey());
    let meta = assert_ok(ctx.try_create_campaign(&creator, &campaign, sol(1), ctx.now() + DAY));
    // No top-up needed: the creator only paid for the campaign account + fee.
    assert_eq!(ctx.balance(&vault), gift);
    assert_eq!(
        ctx.balance(&creator.pubkey()),
        creator_before - ctx.rent_exempt(Campaign::LEN) - meta.fee
    );
}

#[test]
fn succeeds_when_campaign_address_was_prefunded() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let campaign = solana_keypair::Keypair::new();
    // Smallest balance the runtime allows a fresh system account to hold.
    let dust = ctx.rent_exempt(0);
    ctx.svm.airdrop(&campaign.pubkey(), dust).unwrap();

    assert_ok(ctx.try_create_campaign(&creator, &campaign, sol(1), ctx.now() + DAY));
    let account = ctx.svm.get_account(&campaign.pubkey()).unwrap();
    assert_eq!(account.owner, ctx.program_id);
    assert_eq!(account.lamports, ctx.rent_exempt(Campaign::LEN));
}
