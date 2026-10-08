/**
 * Client-side mirror of the on-chain program's wire format: PDA seeds,
 * instruction encoding, account decoding and error codes.
 *
 * Kept dependency-free (manual little-endian Buffer encoding instead of a
 * Borsh library) because the layouts are tiny and fixed-size. Every constant
 * here must match the Rust crate (`src/pda.rs`, `src/instruction.rs`,
 * `src/state.rs`, `src/error.rs`).
 */
import {
  PublicKey,
  SystemProgram,
  TransactionInstruction,
  type AccountMeta,
} from "@solana/web3.js";

// ----- PDAs ------------------------------------------------------------------

export const VAULT_SEED = Buffer.from("vault");
export const CONTRIBUTION_SEED = Buffer.from("contribution");

/** `["vault", campaign]` – system-owned account escrowing donations. */
export function findVaultAddress(programId: PublicKey, campaign: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([VAULT_SEED, campaign.toBuffer()], programId)[0];
}

/** `["contribution", campaign, contributor]` – per-donor accounting record. */
export function findContributionAddress(
  programId: PublicKey,
  campaign: PublicKey,
  contributor: PublicKey,
): PublicKey {
  return PublicKey.findProgramAddressSync(
    [CONTRIBUTION_SEED, campaign.toBuffer(), contributor.toBuffer()],
    programId,
  )[0];
}

// ----- Instructions ------------------------------------------------------------

/** Borsh variant indices of `CrowdfundingInstruction`. */
export enum InstructionTag {
  CreateCampaign = 0,
  Contribute = 1,
  Withdraw = 2,
  Refund = 3,
}

const writable = (pubkey: PublicKey, isSigner = false): AccountMeta => ({
  pubkey,
  isSigner,
  isWritable: true,
});
const systemProgram: AccountMeta = {
  pubkey: SystemProgram.programId,
  isSigner: false,
  isWritable: false,
};

/** `CreateCampaign { goal: u64, deadline: i64 }` – 17 bytes. */
export function createCampaignIx(
  programId: PublicKey,
  creator: PublicKey,
  campaign: PublicKey,
  goal: bigint,
  deadline: bigint,
): TransactionInstruction {
  const data = Buffer.alloc(17);
  data.writeUInt8(InstructionTag.CreateCampaign, 0);
  data.writeBigUInt64LE(goal, 1);
  data.writeBigInt64LE(deadline, 9);
  return new TransactionInstruction({
    programId,
    keys: [
      writable(creator, true),
      writable(campaign, true),
      writable(findVaultAddress(programId, campaign)),
      systemProgram,
    ],
    data,
  });
}

/** `Contribute { amount: u64 }` – 9 bytes. */
export function contributeIx(
  programId: PublicKey,
  contributor: PublicKey,
  campaign: PublicKey,
  amount: bigint,
): TransactionInstruction {
  const data = Buffer.alloc(9);
  data.writeUInt8(InstructionTag.Contribute, 0);
  data.writeBigUInt64LE(amount, 1);
  return new TransactionInstruction({
    programId,
    keys: [
      writable(contributor, true),
      writable(campaign),
      writable(findVaultAddress(programId, campaign)),
      writable(findContributionAddress(programId, campaign, contributor)),
      systemProgram,
    ],
    data,
  });
}

/** `Withdraw` – 1 byte. */
export function withdrawIx(
  programId: PublicKey,
  creator: PublicKey,
  campaign: PublicKey,
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      writable(creator, true),
      writable(campaign),
      writable(findVaultAddress(programId, campaign)),
      systemProgram,
    ],
    data: Buffer.from([InstructionTag.Withdraw]),
  });
}

/** `Refund` – 1 byte. */
export function refundIx(
  programId: PublicKey,
  contributor: PublicKey,
  campaign: PublicKey,
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      writable(contributor, true),
      writable(campaign),
      writable(findVaultAddress(programId, campaign)),
      writable(findContributionAddress(programId, campaign, contributor)),
      systemProgram,
    ],
    data: Buffer.from([InstructionTag.Refund]),
  });
}

// ----- Accounts ----------------------------------------------------------------

/** First byte of every program-owned account (`AccountType` in Rust). */
export enum AccountType {
  Uninitialized = 0,
  Campaign = 1,
  Contribution = 2,
}

/** Discriminator (1) + creator (32) + goal (8) + raised (8) + deadline (8) + claimed (1). */
export const CAMPAIGN_ACCOUNT_LEN = 58;

export interface Campaign {
  creator: PublicKey;
  goal: bigint;
  raised: bigint;
  deadline: bigint;
  claimed: boolean;
}

/** Decodes a campaign account, validating size and discriminator. */
export function decodeCampaign(data: Buffer): Campaign {
  if (data.length !== CAMPAIGN_ACCOUNT_LEN || data.readUInt8(0) !== AccountType.Campaign) {
    throw new Error("Account is not a crowdfunding Campaign");
  }
  return {
    creator: new PublicKey(data.subarray(1, 33)),
    goal: data.readBigUInt64LE(33),
    raised: data.readBigUInt64LE(41),
    deadline: data.readBigInt64LE(49),
    claimed: data.readUInt8(57) !== 0,
  };
}

export type CampaignStatus = "Active" | "Succeeded" | "Claimed" | "Failed";

/** Same lifecycle rules as `Campaign::status` in `src/state.rs`. */
export function campaignStatus(campaign: Campaign, now: bigint): CampaignStatus {
  if (now < campaign.deadline) return "Active";
  if (campaign.claimed) return "Claimed";
  return campaign.raised >= campaign.goal ? "Succeeded" : "Failed";
}

// ----- Errors ------------------------------------------------------------------

/** `CrowdfundingError` variants, indexed by their `ProgramError::Custom` code. */
export const PROGRAM_ERRORS = [
  "InvalidGoal",
  "DeadlineInPast",
  "CampaignEnded",
  "CampaignNotEnded",
  "GoalNotReached",
  "GoalReached",
  "AlreadyClaimed",
  "Unauthorized",
  "InvalidVault",
  "InvalidContributionAccount",
  "NothingToRefund",
  "ZeroAmount",
  "Overflow",
  "AlreadyInitialized",
  "InvalidAccountData",
  "DuplicateAccount",
] as const;

/** Maps a `custom program error: 0x..` code to its variant name, if known. */
export function programErrorName(code: number): string | undefined {
  return PROGRAM_ERRORS[code];
}
