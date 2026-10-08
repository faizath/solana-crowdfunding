//! `Withdraw` behaviour.

use solana_crowdfunding::{error::CrowdfundingError, instruction};
use solana_keypair::Keypair;
use solana_program::{instruction::InstructionError, pubkey::Pubkey};
use solana_signer::Signer;

use crate::common::*;

/// A campaign with `goal` and `raised` lamports contributed by one donor.
fn funded_campaign(goal: u64, raised: u64) -> (TestContext, Keypair, Pubkey, i64) {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let donor = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, goal, deadline).unwrap();
    if raised > 0 {
        assert_ok(ctx.contribute(&donor, &campaign.pubkey(), raised));
    }
    (ctx, creator, campaign.pubkey(), deadline)
}

#[test]
fn pays_entire_vault_to_creator_when_goal_exactly_met_at_deadline() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(2), sol(2));
    ctx.warp_to(deadline);
    let vault = ctx.vault_address(&campaign);
    let expected = sol(2) + ctx.rent_exempt(0);
    let before = ctx.balance(&creator.pubkey());

    let result = ctx.withdraw(&creator, &campaign);
    let meta = assert_logged(&result, &format!("Withdrawn: {expected} lamports"));

    assert_eq!(ctx.balance(&creator.pubkey()), before + expected - meta.fee);
    assert_eq!(ctx.balance(&vault), 0);
    let state = ctx.campaign(&campaign);
    assert!(state.claimed);
    assert_eq!(
        state.raised,
        sol(2),
        "raised is kept as a historical record"
    );
}

#[test]
fn also_sweeps_lamports_sent_directly_to_vault() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(1), sol(1));
    let vault = ctx.vault_address(&campaign);
    ctx.svm.airdrop(&vault, 777).unwrap();
    ctx.warp_to(deadline + 1);

    let expected = sol(1) + ctx.rent_exempt(0) + 777;
    let result = ctx.withdraw(&creator, &campaign);
    assert_logged(&result, &format!("Withdrawn: {expected} lamports"));
    assert_eq!(ctx.balance(&vault), 0);
}

#[test]
fn fails_before_deadline() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(1), sol(1));
    ctx.warp_to(deadline - 1);
    let result = ctx.withdraw(&creator, &campaign);
    assert_custom_error(result, CrowdfundingError::CampaignNotEnded);
}

#[test]
fn fails_when_goal_not_met() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(2), sol(2) - 1);
    ctx.warp_to(deadline + 1);
    let result = ctx.withdraw(&creator, &campaign);
    assert_custom_error(result, CrowdfundingError::GoalNotReached);
}

#[test]
fn fails_when_nothing_was_raised() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(1), 0);
    ctx.warp_to(deadline + 1);
    let result = ctx.withdraw(&creator, &campaign);
    assert_custom_error(result, CrowdfundingError::GoalNotReached);
}

#[test]
fn fails_for_non_creator() {
    let (mut ctx, _, campaign, deadline) = funded_campaign(sol(1), sol(1));
    let thief = ctx.funded_wallet();
    ctx.warp_to(deadline + 1);
    let vault = ctx.vault_address(&campaign);
    let vault_before = ctx.balance(&vault);

    let result = ctx.withdraw(&thief, &campaign);
    assert_custom_error(result, CrowdfundingError::Unauthorized);
    assert_eq!(ctx.balance(&vault), vault_before);
}

#[test]
fn fails_when_already_claimed() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(1), sol(1));
    ctx.warp_to(deadline + 1);
    assert_ok(ctx.withdraw(&creator, &campaign));

    // Even if someone refills the vault, the campaign cannot pay out twice.
    let vault = ctx.vault_address(&campaign);
    ctx.svm.airdrop(&vault, sol(1)).unwrap();
    let result = ctx.withdraw(&creator, &campaign);
    assert_custom_error(result, CrowdfundingError::AlreadyClaimed);
    assert_eq!(ctx.balance(&vault), sol(1));
}

#[test]
fn requires_creator_signature() {
    let (mut ctx, creator, campaign, deadline) = funded_campaign(sol(1), sol(1));
    let fee_payer = ctx.funded_wallet();
    ctx.warp_to(deadline + 1);

    let mut ix = instruction::withdraw(&ctx.program_id, &creator.pubkey(), &campaign);
    ix.accounts[0].is_signer = false;
    let result = ctx.send(&[ix], &[&fee_payer]);
    assert_instruction_error(result, InstructionError::MissingRequiredSignature);
}
