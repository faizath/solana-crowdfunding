//! `Refund` behaviour.

use solana_crowdfunding::{
    error::CrowdfundingError,
    instruction,
    state::{AccountState, Contribution},
};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;

use crate::common::*;

struct FailedCampaign {
    ctx: TestContext,
    campaign: Pubkey,
    deadline: i64,
    alice: Keypair,
    bob: Keypair,
}

/// Goal 10 SOL; Alice gives 3 SOL, Bob gives 1 SOL twice. Still active.
fn underfunded_campaign() -> FailedCampaign {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let alice = ctx.funded_wallet();
    let bob = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, sol(10), deadline).unwrap();
    let campaign = campaign.pubkey();
    assert_ok(ctx.contribute(&alice, &campaign, sol(3)));
    assert_ok(ctx.contribute(&bob, &campaign, sol(1)));
    assert_ok(ctx.contribute(&bob, &campaign, sol(1)));
    FailedCampaign {
        ctx,
        campaign,
        deadline,
        alice,
        bob,
    }
}

#[test]
fn returns_each_contributor_exactly_their_contribution() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        alice,
        bob,
    } = underfunded_campaign();
    ctx.warp_to(deadline + 1);
    let vault = ctx.vault_address(&campaign);
    let contribution_rent = ctx.rent_exempt(Contribution::LEN);

    // Alice: 3 SOL back plus the rent of her closed contribution account.
    let before = ctx.balance(&alice.pubkey());
    let result = ctx.refund(&alice, &campaign);
    let meta = assert_logged(&result, &format!("Refunded: {} lamports", sol(3)));
    assert_eq!(
        ctx.balance(&alice.pubkey()),
        before + sol(3) + contribution_rent - meta.fee
    );
    assert!(ctx.contribution(&campaign, &alice.pubkey()).is_none());
    assert_eq!(ctx.campaign(&campaign).raised, sol(2));

    // Bob: both of his contributions (2 SOL) in a single refund.
    let before = ctx.balance(&bob.pubkey());
    let result = ctx.refund(&bob, &campaign);
    let meta = assert_logged(&result, &format!("Refunded: {} lamports", sol(2)));
    assert_eq!(
        ctx.balance(&bob.pubkey()),
        before + sol(2) + contribution_rent - meta.fee
    );
    assert!(ctx.contribution(&campaign, &bob.pubkey()).is_none());

    // Accounting stays consistent: nothing outstanding, only the reserve left.
    assert_eq!(ctx.campaign(&campaign).raised, 0);
    assert_eq!(ctx.balance(&vault), ctx.rent_exempt(0));
}

#[test]
fn closes_contribution_account_completely() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        alice,
        ..
    } = underfunded_campaign();
    ctx.warp_to(deadline);
    assert_ok(ctx.refund(&alice, &campaign));

    let address = ctx.contribution_address(&campaign, &alice.pubkey());
    let closed = ctx.svm.get_account(&address);
    assert!(
        closed
            .as_ref()
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()),
        "contribution account should be closed, got {closed:?}"
    );
}

#[test]
fn fails_before_deadline() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        alice,
        ..
    } = underfunded_campaign();
    ctx.warp_to(deadline - 1);
    let result = ctx.refund(&alice, &campaign);
    assert_custom_error(result, CrowdfundingError::CampaignNotEnded);
}

#[test]
fn fails_when_goal_met() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        alice,
        ..
    } = underfunded_campaign();
    // Top the campaign up to exactly its goal (5 SOL raised so far).
    assert_ok(ctx.contribute(&alice, &campaign, sol(5)));
    ctx.warp_to(deadline + 1);
    let result = ctx.refund(&alice, &campaign);
    assert_custom_error(result, CrowdfundingError::GoalReached);
}

#[test]
fn fails_on_double_refund() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        alice,
        ..
    } = underfunded_campaign();
    ctx.warp_to(deadline + 1);
    assert_ok(ctx.refund(&alice, &campaign));

    let vault = ctx.vault_address(&campaign);
    let vault_before = ctx.balance(&vault);
    let result = ctx.refund(&alice, &campaign);
    assert_custom_error(result, CrowdfundingError::NothingToRefund);
    assert_eq!(ctx.balance(&vault), vault_before);
}

#[test]
fn fails_for_non_contributor() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        ..
    } = underfunded_campaign();
    let stranger = ctx.funded_wallet();
    ctx.warp_to(deadline + 1);
    let result = ctx.refund(&stranger, &campaign);
    assert_custom_error(result, CrowdfundingError::NothingToRefund);
}

#[test]
fn cannot_claim_someone_elses_contribution() {
    let FailedCampaign {
        mut ctx,
        campaign,
        deadline,
        alice,
        ..
    } = underfunded_campaign();
    let thief = ctx.funded_wallet();
    ctx.warp_to(deadline + 1);

    let mut ix = instruction::refund(&ctx.program_id, &thief.pubkey(), &campaign);
    ix.accounts[3].pubkey = ctx.contribution_address(&campaign, &alice.pubkey());
    let result = ctx.send(&[ix], &[&thief]);
    assert_custom_error(result, CrowdfundingError::InvalidContributionAccount);
    assert_eq!(
        ctx.contribution(&campaign, &alice.pubkey()).unwrap().amount,
        sol(3)
    );
}

#[test]
fn creator_cannot_withdraw_failed_campaign() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, sol(10), deadline).unwrap();
    assert_ok(ctx.contribute(&creator, &campaign.pubkey(), sol(1)));
    ctx.warp_to(deadline + 1);

    let result = ctx.withdraw(&creator, &campaign.pubkey());
    assert_custom_error(result, CrowdfundingError::GoalNotReached);
    // ...but as a donor the creator can still refund their own contribution.
    assert_ok(ctx.refund(&creator, &campaign.pubkey()));
}
