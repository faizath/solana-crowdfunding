//! Adversarial inputs: forged accounts, wrong PDAs, missing signatures,
//! type confusion and griefing attempts.

use solana_account::Account;
use solana_crowdfunding::{
    error::CrowdfundingError,
    instruction,
    state::{AccountState, Campaign},
};
use solana_keypair::Keypair;
use solana_program::{
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
};
use solana_signer::Signer;

use crate::common::*;

/// Index of each account in the `Contribute` / `Refund` account lists.
const CAMPAIGN: usize = 1;
const VAULT: usize = 2;
const CONTRIBUTION: usize = 3;
const SYSTEM_PROGRAM: usize = 4;

struct Setup {
    ctx: TestContext,
    creator: Keypair,
    donor: Keypair,
    campaign: Pubkey,
    deadline: i64,
}

fn setup() -> Setup {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let donor = ctx.funded_wallet();
    let deadline = ctx.now() + DAY;
    let (campaign, _) = ctx.create_campaign(&creator, sol(1), deadline).unwrap();
    Setup {
        ctx,
        creator,
        donor,
        campaign: campaign.pubkey(),
        deadline,
    }
}

/// Installs a forged campaign account with attacker-chosen state and owner.
fn forge_campaign(ctx: &mut TestContext, owner: Pubkey, state: &Campaign) -> Pubkey {
    let mut data = vec![0u8; Campaign::LEN];
    state.pack(&mut data).unwrap();
    let address = Pubkey::new_unique();
    let account = Account {
        lamports: ctx.rent_exempt(Campaign::LEN),
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    };
    ctx.svm.set_account(address, account).unwrap();
    address
}

// ----- PDA validation ----------------------------------------------------------

#[test]
fn contribute_rejects_wrong_vault() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, sol(1));
    ix.accounts[VAULT].pubkey = donor.pubkey();
    // Donor as "vault" is also writable & system-owned, so only the PDA check
    // stands between the attacker and a self-transfer that inflates `raised`.
    ix.accounts[VAULT].is_writable = true;
    let result = ctx.send(&[ix], &[&donor]);
    assert_custom_error(result, CrowdfundingError::InvalidVault);
}

#[test]
fn contribute_rejects_vault_of_another_campaign() {
    let Setup {
        mut ctx,
        creator,
        donor,
        campaign,
        deadline,
    } = setup();
    let (other, _) = ctx.create_campaign(&creator, sol(1), deadline).unwrap();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, sol(1));
    ix.accounts[VAULT].pubkey = ctx.vault_address(&other.pubkey());
    let result = ctx.send(&[ix], &[&donor]);
    assert_custom_error(result, CrowdfundingError::InvalidVault);
}

#[test]
fn withdraw_rejects_vault_of_another_campaign() {
    let Setup {
        mut ctx,
        creator,
        donor,
        campaign,
        deadline,
    } = setup();
    // A second, *richer* campaign by someone else.
    let other_creator = ctx.funded_wallet();
    let (rich, _) = ctx
        .create_campaign(&other_creator, sol(1), deadline)
        .unwrap();
    assert_ok(ctx.contribute(&donor, &campaign, sol(1)));
    assert_ok(ctx.contribute(&donor, &rich.pubkey(), sol(50)));
    ctx.warp_to(deadline + 1);

    let mut ix = instruction::withdraw(&ctx.program_id, &creator.pubkey(), &campaign);
    ix.accounts[VAULT].pubkey = ctx.vault_address(&rich.pubkey());
    let result = ctx.send(&[ix], &[&creator]);
    assert_custom_error(result, CrowdfundingError::InvalidVault);
}

#[test]
fn create_rejects_wrong_vault() {
    let mut ctx = TestContext::new();
    let creator = ctx.funded_wallet();
    let campaign = Keypair::new();
    let mut ix = instruction::create_campaign(
        &ctx.program_id,
        &creator.pubkey(),
        &campaign.pubkey(),
        sol(1),
        ctx.now() + DAY,
    );
    ix.accounts[2].pubkey = Pubkey::new_unique();
    let result = ctx.send(&[ix], &[&creator, &campaign]);
    assert_custom_error(result, CrowdfundingError::InvalidVault);
}

#[test]
fn contribute_rejects_non_pda_contribution_account() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, sol(1));
    ix.accounts[CONTRIBUTION].pubkey = Pubkey::new_unique();
    let result = ctx.send(&[ix], &[&donor]);
    assert_custom_error(result, CrowdfundingError::InvalidContributionAccount);
}

#[test]
fn contribute_rejects_someone_elses_contribution_account() {
    let Setup {
        mut ctx,
        creator,
        donor,
        campaign,
        ..
    } = setup();
    assert_ok(ctx.contribute(&creator, &campaign, sol(1)));

    // Donor tries to credit their contribution to the creator's record.
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, 1);
    ix.accounts[CONTRIBUTION].pubkey = ctx.contribution_address(&campaign, &creator.pubkey());
    let result = ctx.send(&[ix], &[&donor]);
    assert_custom_error(result, CrowdfundingError::InvalidContributionAccount);
}

#[test]
fn refund_rejects_contribution_pda_of_another_campaign() {
    let Setup {
        mut ctx,
        creator,
        donor,
        campaign,
        deadline,
    } = setup();
    let (other, _) = ctx.create_campaign(&creator, sol(100), deadline).unwrap();
    assert_ok(ctx.contribute(&donor, &campaign, 10));
    assert_ok(ctx.contribute(&donor, &other.pubkey(), sol(5)));
    ctx.warp_to(deadline + 1);

    // Refund from the small campaign using the big campaign's record.
    let mut ix = instruction::refund(&ctx.program_id, &donor.pubkey(), &campaign);
    ix.accounts[CONTRIBUTION].pubkey = ctx.contribution_address(&other.pubkey(), &donor.pubkey());
    let result = ctx.send(&[ix], &[&donor]);
    assert_custom_error(result, CrowdfundingError::InvalidContributionAccount);
}

// ----- ownership & type confusion ----------------------------------------------

#[test]
fn rejects_campaign_not_owned_by_program() {
    let Setup { mut ctx, donor, .. } = setup();
    let attacker = ctx.funded_wallet();
    let now = ctx.now();
    let fake = forge_campaign(
        &mut ctx,
        Pubkey::new_unique(),
        &Campaign {
            creator: attacker.pubkey(),
            goal: 1,
            raised: u64::MAX / 2,
            deadline: now + DAY,
            claimed: false,
        },
    );

    let result = ctx.contribute(&donor, &fake, 1);
    assert_instruction_error(result, InstructionError::IncorrectProgramId);

    ctx.warp_to(now + 2 * DAY);
    let result = ctx.withdraw(&attacker, &fake);
    assert_instruction_error(result, InstructionError::IncorrectProgramId);
}

#[test]
fn rejects_system_owned_campaign_with_valid_looking_data() {
    let Setup {
        mut ctx,
        donor,
        deadline,
        ..
    } = setup();
    let fake = forge_campaign(
        &mut ctx,
        solana_system_interface::program::ID,
        &Campaign {
            creator: donor.pubkey(),
            goal: 1,
            raised: 0,
            deadline,
            claimed: false,
        },
    );
    let result = ctx.contribute(&donor, &fake, 1);
    assert_instruction_error(result, InstructionError::IncorrectProgramId);
}

#[test]
fn rejects_contribution_account_passed_as_campaign() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    assert_ok(ctx.contribute(&donor, &campaign, 1));
    let record = ctx.contribution_address(&campaign, &donor.pubkey());

    // Program-owned but the wrong account type: the discriminator check fails.
    let result = ctx.contribute(&donor, &record, 1);
    assert_custom_error(result, CrowdfundingError::InvalidAccountData);
}

// ----- program ids, signers, writability ---------------------------------------

#[test]
fn rejects_fake_system_program() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, 1);
    ix.accounts[SYSTEM_PROGRAM].pubkey = ctx.program_id;
    let result = ctx.send(&[ix], &[&donor]);
    assert_instruction_error(result, InstructionError::IncorrectProgramId);
}

#[test]
fn contribute_requires_contributor_signature() {
    let Setup {
        mut ctx,
        creator,
        donor,
        campaign,
        ..
    } = setup();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, sol(1));
    ix.accounts[0].is_signer = false;
    let result = ctx.send(&[ix], &[&creator]);
    assert_instruction_error(result, InstructionError::MissingRequiredSignature);
}

#[test]
fn refund_requires_contributor_signature() {
    let Setup {
        mut ctx,
        creator,
        donor,
        campaign,
        deadline,
    } = setup();
    assert_ok(ctx.contribute(&donor, &campaign, 10));
    ctx.warp_to(deadline + 1);
    let mut ix = instruction::refund(&ctx.program_id, &donor.pubkey(), &campaign);
    ix.accounts[0].is_signer = false;
    let result = ctx.send(&[ix], &[&creator]);
    assert_instruction_error(result, InstructionError::MissingRequiredSignature);
}

#[test]
fn rejects_read_only_campaign() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, 1);
    ix.accounts[CAMPAIGN].is_writable = false;
    let result = ctx.send(&[ix], &[&donor]);
    assert_instruction_error(result, InstructionError::InvalidAccountData);
}

#[test]
// `next_account_info` returns `ProgramError::NotEnoughAccountKeys`, which the
// runtime still maps to this (deprecated) `InstructionError` variant.
#[allow(deprecated)]
fn rejects_missing_accounts() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    let mut ix = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, 1);
    ix.accounts.truncate(3);
    let result = ctx.send(&[ix], &[&donor]);
    assert_instruction_error(result, InstructionError::NotEnoughAccountKeys);
}

#[test]
fn rejects_malformed_instruction_data() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    let template = instruction::contribute(&ctx.program_id, &donor.pubkey(), &campaign, 1);
    for data in [vec![], vec![42], vec![1, 0, 0], vec![2, 0xff]] {
        let ix = Instruction {
            data,
            ..template.clone()
        };
        let result = ctx.send(&[ix], &[&donor]);
        assert_instruction_error(result, InstructionError::InvalidInstructionData);
    }
}

// ----- griefing ------------------------------------------------------------------

#[test]
fn prefunded_contribution_pda_cannot_block_donor() {
    let Setup {
        mut ctx,
        donor,
        campaign,
        ..
    } = setup();
    // An attacker "dusts" the donor's future contribution PDA so that a naive
    // `create_account` would fail with `AccountAlreadyInUse`.
    let address = ctx.contribution_address(&campaign, &donor.pubkey());
    let dust = ctx.rent_exempt(0);
    ctx.svm.airdrop(&address, dust).unwrap();

    let result = ctx.contribute(&donor, &campaign, sol(1));
    assert_logged(
        &result,
        &format!("Contributed: {} lamports, total={}", sol(1), sol(1)),
    );
    assert_eq!(
        ctx.contribution(&campaign, &donor.pubkey()).unwrap().amount,
        sol(1)
    );
}
