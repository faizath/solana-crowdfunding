//! # Solana Crowdfunding
//!
//! A Kickstarter-style crowdfunding program written as a native Solana program
//! (no Anchor). Creators open campaigns with a lamport `goal` and a unix
//! `deadline`; donors contribute SOL that is escrowed in a program-derived
//! **vault** rather than sent to the creator. After the deadline exactly one of
//! two outcomes is possible:
//!
//! * **Success** (`raised >= goal`): the creator calls
//!   [`Withdraw`](instruction::CrowdfundingInstruction::Withdraw) once and
//!   receives every lamport held by the vault.
//! * **Failure** (`raised < goal`): every donor can call
//!   [`Refund`](instruction::CrowdfundingInstruction::Refund) to get back
//!   exactly what they contributed, tracked in a per-donor
//!   [`Contribution`](state::Contribution) PDA.
//!
//! ## Crate layout
//!
//! | Module          | Responsibility                                               |
//! |-----------------|--------------------------------------------------------------|
//! | [`entrypoint`]  | BPF entrypoint, forwards to [`processor::process_instruction`] |
//! | [`instruction`] | Borsh instruction enum + client-side `Instruction` builders  |
//! | [`processor`]   | One handler per instruction                                  |
//! | [`state`]       | On-chain account layouts with type discriminators            |
//! | [`pda`]         | Seed constants and address derivation helpers                |
//! | [`validation`]  | Reusable account-validation guards                           |
//! | [`error`]       | [`CrowdfundingError`](error::CrowdfundingError) custom errors |
//!
//! Build the `no-entrypoint` feature to use this crate as a client library
//! (instruction builders, state decoding, PDA helpers) from another program or
//! from off-chain Rust code.

#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod instruction;
pub mod pda;
pub mod processor;
pub mod state;
pub mod validation;

solana_program::declare_id!("8161wQkF9HFd9idyE4AzNQFkWQk2KqxZwiswxviYL9g5");
