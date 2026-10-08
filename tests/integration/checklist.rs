//! The project brief's "Testing Checklist", executed end to end.

use solana_crowdfunding::error::CrowdfundingError;
use solana_signer::Signer;

use crate::common::*;

#[test]
fn brief_testing_checklist_end_to_end() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let donor_a = ctx.funded_wallet();
    let donor_b = ctx.funded_wallet();

    // 1. Create a campaign with goal = 1000 SOL, deadline = tomorrow.
    let goal = sol(1_000);
    let deadline = ctx.now() + DAY;
    let (campaign_kp, meta) = ctx
        .create_campaign(&creator, goal, deadline)
        .unwrap_or_else(|f| panic!("create failed: {:?}\n{:#?}", f.err, f.meta.logs));
    assert_log_line(
        &meta,
        &format!("Campaign created: goal={goal}, deadline={deadline}"),
    );
    let campaign = campaign_kp.pubkey();
    let state = ctx.campaign(&campaign);
    assert_eq!(state.creator, creator.pubkey());
    assert_eq!(state.goal, goal);
    assert_eq!(state.raised, 0);
    assert_eq!(state.deadline, deadline);
    assert!(!state.claimed);

    // 2. Contribute 600 SOL -> succeeds, raised = 600.
    let result = ctx.contribute(&donor_a, &campaign, sol(600));
    assert_logged(
        &result,
        &format!("Contributed: {} lamports, total={}", sol(600), sol(600)),
    );
    assert_eq!(ctx.campaign(&campaign).raised, sol(600));

    // 3. Contribute 500 SOL -> succeeds, raised = 1100.
    let result = ctx.contribute(&donor_b, &campaign, sol(500));
    assert_logged(
        &result,
        &format!("Contributed: {} lamports, total={}", sol(500), sol(1_100)),
    );
    assert_eq!(ctx.campaign(&campaign).raised, sol(1_100));

    // 4. Try withdraw before the deadline -> fails.
    let result = ctx.withdraw(&creator, &campaign);
    assert_custom_error(result, CrowdfundingError::CampaignNotEnded);

    // 5. After the deadline -> withdraw succeeds and the creator gets the funds.
    ctx.warp_to(deadline + 1);
    let vault = ctx.vault_address(&campaign);
    let vault_balance = ctx.balance(&vault);
    assert_eq!(vault_balance, sol(1_100) + ctx.rent_exempt(0));
    let creator_before = ctx.balance(&creator.pubkey());

    let result = ctx.withdraw(&creator, &campaign);
    let meta = assert_logged(&result, &format!("Withdrawn: {vault_balance} lamports"));
    assert_eq!(
        ctx.balance(&creator.pubkey()),
        creator_before + vault_balance - meta.fee
    );
    assert_eq!(ctx.balance(&vault), 0);
    assert!(ctx.campaign(&campaign).claimed);

    // 6. Try withdraw again -> fails (already claimed).
    let result = ctx.withdraw(&creator, &campaign);
    assert_custom_error(result, CrowdfundingError::AlreadyClaimed);
}
