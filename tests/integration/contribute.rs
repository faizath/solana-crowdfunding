//! `Contribute` behaviour.

use solana_account::Account;
use solana_crowdfunding::{
    error::CrowdfundingError,
    state::{AccountState, Campaign, Contribution},
};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_signer::Signer;

use crate::common::*;

fn setup(goal: u64) -> (TestContext, Keypair, Pubkey, i64) {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, goal, deadline).unwrap();
    (ctx, creator, campaign.pubkey(), deadline)
}

#[test]
fn moves_lamports_into_vault_and_records_contribution() {
    let (mut ctx, _, campaign, _) = setup(sol(10));
    let donor = ctx.funded_wallet();
    let vault = ctx.vault_address(&campaign);
    let vault_before = ctx.balance(&vault);
    let donor_before = ctx.balance(&donor.pubkey());

    let meta = assert_ok(ctx.contribute(&donor, &campaign, sol(3)));

    assert_eq!(ctx.balance(&vault), vault_before + sol(3));
    assert_eq!(
        ctx.balance(&donor.pubkey()),
        donor_before - sol(3) - ctx.rent_exempt(Contribution::LEN) - meta.fee
    );
    assert_eq!(ctx.campaign(&campaign).raised, sol(3));

    let record = ctx.contribution(&campaign, &donor.pubkey()).unwrap();
    assert_eq!(record.campaign, campaign);
    assert_eq!(record.contributor, donor.pubkey());
    assert_eq!(record.amount, sol(3));
    let (_, bump) = solana_crowdfunding::pda::find_contribution_address(
        &ctx.program_id,
        &campaign,
        &donor.pubkey(),
    );
    assert_eq!(record.bump, bump);
}

#[test]
fn logs_exact_message() {
    let (mut ctx, _, campaign, _) = setup(sol(10));
    let donor = ctx.funded_wallet();
    ctx.contribute(&donor, &campaign, 400).unwrap();
    let result = ctx.contribute(&donor, &campaign, 600);
    assert_logged(&result, "Contributed: 600 lamports, total=1000");
}

#[test]
fn same_donor_contributions_accumulate() {
    let (mut ctx, _, campaign, _) = setup(sol(10));
    let donor = ctx.funded_wallet();

    assert_ok(ctx.contribute(&donor, &campaign, sol(1)));
    let after_first = ctx.balance(&donor.pubkey());
    let second = assert_ok(ctx.contribute(&donor, &campaign, sol(2)));
    let third = assert_logged(
        &ctx.contribute(&donor, &campaign, sol(3)),
        &format!("Contributed: {} lamports, total={}", sol(3), sol(6)),
    );

    assert_eq!(
        ctx.contribution(&campaign, &donor.pubkey()).unwrap().amount,
        sol(6)
    );
    assert_eq!(ctx.campaign(&campaign).raised, sol(6));
    // Rent for the contribution PDA is only paid on the first contribution.
    assert_eq!(
        after_first - ctx.balance(&donor.pubkey()),
        sol(5) + second.fee + third.fee
    );
}

#[test]
fn tracks_each_donor_separately() {
    let (mut ctx, _, campaign, _) = setup(sol(10));
    let alice = ctx.funded_wallet();
    let bob = ctx.funded_wallet();
    ctx.contribute(&alice, &campaign, sol(1)).unwrap();
    ctx.contribute(&bob, &campaign, sol(2)).unwrap();
    ctx.contribute(&alice, &campaign, sol(4)).unwrap();

    assert_eq!(
        ctx.contribution(&campaign, &alice.pubkey()).unwrap().amount,
        sol(5)
    );
    assert_eq!(
        ctx.contribution(&campaign, &bob.pubkey()).unwrap().amount,
        sol(2)
    );
    assert_eq!(ctx.campaign(&campaign).raised, sol(7));
}

#[test]
fn accepts_contributions_beyond_goal() {
    let (mut ctx, _, campaign, _) = setup(sol(1));
    let donor = ctx.funded_wallet();
    assert_ok(ctx.contribute(&donor, &campaign, sol(1)));
    assert_ok(ctx.contribute(&donor, &campaign, sol(2)));
    assert_eq!(ctx.campaign(&campaign).raised, sol(3));
}

#[test]
fn accepts_single_lamport_thanks_to_rent_reserve() {
    let (mut ctx, _, campaign, _) = setup(sol(1));
    let donor = ctx.funded_wallet();
    assert_logged(
        &ctx.contribute(&donor, &campaign, 1),
        "Contributed: 1 lamports, total=1",
    );
}

#[test]
fn rejects_zero_amount() {
    let (mut ctx, _, campaign, _) = setup(sol(1));
    let donor = ctx.funded_wallet();
    let result = ctx.contribute(&donor, &campaign, 0);
    assert_custom_error(result, CrowdfundingError::ZeroAmount);
}

#[test]
fn accepts_one_second_before_deadline() {
    let (mut ctx, _, campaign, deadline) = setup(sol(1));
    let donor = ctx.funded_wallet();
    ctx.warp_to(deadline - 1);
    assert_ok(ctx.contribute(&donor, &campaign, 1));
}

#[test]
fn rejects_at_deadline() {
    let (mut ctx, _, campaign, deadline) = setup(sol(1));
    let donor = ctx.funded_wallet();
    ctx.warp_to(deadline);
    let result = ctx.contribute(&donor, &campaign, sol(1));
    assert_custom_error(result, CrowdfundingError::CampaignEnded);
}

#[test]
fn rejects_after_deadline() {
    let (mut ctx, _, campaign, deadline) = setup(sol(1));
    let donor = ctx.funded_wallet();
    ctx.warp_to(deadline + DAY);
    let result = ctx.contribute(&donor, &campaign, sol(1));
    assert_custom_error(result, CrowdfundingError::CampaignEnded);
}

#[test]
fn rejects_when_raised_would_overflow() {
    let (mut ctx, _, campaign, _) = setup(sol(1));
    let donor = ctx.funded_wallet();

    // Forge a campaign whose `raised` is at the edge of u64 to exercise the
    // checked arithmetic (unreachable with real lamport supply).
    let mut state = ctx.campaign(&campaign);
    state.raised = u64::MAX - 1;
    let mut account: Account = ctx.svm.get_account(&campaign).unwrap();
    let mut data = vec![0u8; Campaign::LEN];
    state.pack(&mut data).unwrap();
    account.data = data;
    ctx.svm.set_account(campaign, account).unwrap();

    let result = ctx.contribute(&donor, &campaign, 2);
    assert_custom_error(result, CrowdfundingError::Overflow);
}

#[test]
fn direct_vault_transfers_do_not_count_towards_goal() {
    let (mut ctx, _, campaign, _) = setup(sol(1));
    let vault = ctx.vault_address(&campaign);
    ctx.svm.airdrop(&vault, sol(5)).unwrap();
    assert_eq!(ctx.campaign(&campaign).raised, 0);
}
