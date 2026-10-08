//! Integration tests for the crowdfunding program.
//!
//! Everything is compiled into a single test binary (one LiteSVM link instead
//! of one per file) and runs against the real SBF artifact. Build it first:
//!
//! ```sh
//! cargo build-sbf && cargo test
//! ```

// The harness passes LiteSVM's own `TransactionResult` through unchanged.
#![allow(clippy::result_large_err)]

mod checklist;
mod common;
mod contribute;
mod create_campaign;
mod refund;
mod security;
mod withdraw;
