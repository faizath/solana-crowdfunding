//! Shared LiteSVM harness for the integration tests.
//!
//! The tests run the **compiled SBF program** (`target/deploy/solana_crowdfunding.so`)
//! inside LiteSVM, so `cargo build-sbf` must run before `cargo test`.

use std::path::PathBuf;

use litesvm::{
    types::{FailedTransactionMetadata, TransactionMetadata, TransactionResult},
    LiteSVM,
};
use solana_crowdfunding::{
    error::CrowdfundingError,
    instruction,
    pda::{find_contribution_address, find_vault_address},
    state::{AccountState, Campaign, Contribution},
};
use solana_keypair::Keypair;
use solana_program::{
    clock::Clock,
    instruction::{Instruction, InstructionError},
    pubkey::Pubkey,
};
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;

pub use solana_native_token::LAMPORTS_PER_SOL;

/// Fixed "current time" every test starts from (2025-10-08T00:00:00Z), so
/// deadlines are deterministic.
pub const START_TIME: i64 = 1_759_881_600;
/// One day in seconds.
pub const DAY: i64 = 86_400;
/// Default balance given to every funded test wallet.
pub const WALLET_BALANCE: u64 = 10_000 * LAMPORTS_PER_SOL;

/// Converts whole SOL to lamports.
pub const fn sol(amount: u64) -> u64 {
    amount * LAMPORTS_PER_SOL
}

/// A LiteSVM instance with the crowdfunding program loaded.
pub struct TestContext {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
}

impl TestContext {
    pub fn new() -> Self {
        let program_id = solana_crowdfunding::id();
        let mut svm = LiteSVM::new();
        let so_path = program_so_path();
        svm.add_program_from_file(program_id, &so_path)
            .unwrap_or_else(|e| {
                panic!(
                    "failed to load {} ({e:?}); run `cargo build-sbf` before `cargo test`",
                    so_path.display()
                )
            });
        let mut ctx = Self { svm, program_id };
        ctx.warp_to(START_TIME);
        ctx
    }

    // ----- clock ---------------------------------------------------------

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    /// Moves the on-chain `Clock::unix_timestamp` to `unix_timestamp`.
    pub fn warp_to(&mut self, unix_timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar(&clock);
    }

    // ----- wallets & balances ----------------------------------------------

    pub fn funded_wallet(&mut self) -> Keypair {
        self.funded_wallet_with(WALLET_BALANCE)
    }

    pub fn funded_wallet_with(&mut self, lamports: u64) -> Keypair {
        let kp = Keypair::new();
        self.svm
            .airdrop(&kp.pubkey(), lamports)
            .expect("airdrop failed");
        kp
    }

    pub fn balance(&self, address: &Pubkey) -> u64 {
        self.svm.get_balance(address).unwrap_or(0)
    }

    pub fn rent_exempt(&self, data_len: usize) -> u64 {
        self.svm.minimum_balance_for_rent_exemption(data_len)
    }

    // ----- transactions ------------------------------------------------------

    /// Signs and sends `ixs` with `signers[0]` as fee payer.
    ///
    /// The blockhash is rotated afterwards so that byte-identical transactions
    /// (e.g. a second `Withdraw`) are not rejected as duplicates.
    pub fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> TransactionResult {
        let tx = Transaction::new_signed_with_payer(
            ixs,
            Some(&signers[0].pubkey()),
            signers,
            self.svm.latest_blockhash(),
        );
        let result = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        result
    }

    // ----- instruction shortcuts -------------------------------------------

    /// Creates a campaign and returns its keypair.
    pub fn create_campaign(
        &mut self,
        creator: &Keypair,
        goal: u64,
        deadline: i64,
    ) -> Result<(Keypair, TransactionMetadata), FailedTransactionMetadata> {
        let campaign = Keypair::new();
        let meta = self.try_create_campaign(creator, &campaign, goal, deadline)?;
        Ok((campaign, meta))
    }

    pub fn try_create_campaign(
        &mut self,
        creator: &Keypair,
        campaign: &Keypair,
        goal: u64,
        deadline: i64,
    ) -> TransactionResult {
        let ix = instruction::create_campaign(
            &self.program_id,
            &creator.pubkey(),
            &campaign.pubkey(),
            goal,
            deadline,
        );
        self.send(&[ix], &[creator, campaign])
    }

    pub fn contribute(
        &mut self,
        contributor: &Keypair,
        campaign: &Pubkey,
        amount: u64,
    ) -> TransactionResult {
        let ix = instruction::contribute(&self.program_id, &contributor.pubkey(), campaign, amount);
        self.send(&[ix], &[contributor])
    }

    pub fn withdraw(&mut self, creator: &Keypair, campaign: &Pubkey) -> TransactionResult {
        let ix = instruction::withdraw(&self.program_id, &creator.pubkey(), campaign);
        self.send(&[ix], &[creator])
    }

    pub fn refund(&mut self, contributor: &Keypair, campaign: &Pubkey) -> TransactionResult {
        let ix = instruction::refund(&self.program_id, &contributor.pubkey(), campaign);
        self.send(&[ix], &[contributor])
    }

    // ----- state -------------------------------------------------------------

    pub fn campaign(&self, campaign: &Pubkey) -> Campaign {
        let account = self.svm.get_account(campaign).expect("campaign missing");
        assert_eq!(account.owner, self.program_id);
        Campaign::unpack(&account.data).expect("campaign decode")
    }

    /// Returns `None` if the contribution PDA does not exist (never created or
    /// closed by a refund).
    pub fn contribution(&self, campaign: &Pubkey, contributor: &Pubkey) -> Option<Contribution> {
        let address = self.contribution_address(campaign, contributor);
        let account = self.svm.get_account(&address)?;
        if account.lamports == 0 && account.data.is_empty() {
            return None;
        }
        assert_eq!(account.owner, self.program_id);
        Some(Contribution::unpack(&account.data).expect("contribution decode"))
    }

    pub fn vault_address(&self, campaign: &Pubkey) -> Pubkey {
        find_vault_address(&self.program_id, campaign).0
    }

    pub fn contribution_address(&self, campaign: &Pubkey, contributor: &Pubkey) -> Pubkey {
        find_contribution_address(&self.program_id, campaign, contributor).0
    }
}

fn program_so_path() -> PathBuf {
    std::env::var_os("SBF_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/deploy"))
        .join("solana_crowdfunding.so")
}

// ----- assertions --------------------------------------------------------------

/// Asserts the transaction failed in its first instruction with `expected`.
#[track_caller]
pub fn assert_instruction_error(result: TransactionResult, expected: InstructionError) {
    match result {
        Ok(meta) => panic!(
            "expected {expected:?}, transaction succeeded: {:#?}",
            meta.logs
        ),
        Err(failed) => assert_eq!(
            failed.err,
            TransactionError::InstructionError(0, expected),
            "logs: {:#?}",
            failed.meta.logs
        ),
    }
}

/// Asserts the transaction failed with the given custom program error.
#[track_caller]
pub fn assert_custom_error(result: TransactionResult, expected: CrowdfundingError) {
    assert_instruction_error(result, InstructionError::Custom(expected as u32));
}

/// Asserts the transaction succeeded and emitted `expected` via `msg!`.
#[track_caller]
pub fn assert_logged(result: &TransactionResult, expected: &str) -> TransactionMetadata {
    let meta = match result {
        Ok(meta) => meta.clone(),
        Err(failed) => panic!(
            "transaction failed: {:?}\n{:#?}",
            failed.err, failed.meta.logs
        ),
    };
    assert_log_line(&meta, expected);
    meta
}

/// Asserts `meta` contains the program log line `expected`.
#[track_caller]
pub fn assert_log_line(meta: &TransactionMetadata, expected: &str) {
    let line = format!("Program log: {expected}");
    assert!(
        meta.logs.contains(&line),
        "missing log line {line:?} in {:#?}",
        meta.logs
    );
}

/// Unwraps a successful transaction, printing logs on failure.
#[track_caller]
pub fn assert_ok(result: TransactionResult) -> TransactionMetadata {
    match result {
        Ok(meta) => meta,
        Err(failed) => panic!(
            "transaction failed: {:?}\n{:#?}",
            failed.err, failed.meta.logs
        ),
    }
}
