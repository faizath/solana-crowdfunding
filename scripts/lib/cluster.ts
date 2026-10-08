/**
 * Helpers for talking to the cluster: sending transactions with readable
 * errors, following cluster time, and pretty-printing campaign state.
 */
import {
  Connection,
  LAMPORTS_PER_SOL,
  PublicKey,
  SendTransactionError,
  Transaction,
  sendAndConfirmTransaction,
  type Signer,
  type TransactionInstruction,
} from "@solana/web3.js";
import { explorerTxUrl } from "./config.js";
import { campaignStatus, decodeCampaign, findVaultAddress, programErrorName } from "./program.js";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export const formatSol = (lamports: bigint | number): string =>
  `${(Number(lamports) / LAMPORTS_PER_SOL).toFixed(9)} SOL`;

/**
 * Sends `ix` signed by `signers` (first = fee payer), waits for confirmation,
 * and prints the signature, Explorer link and the program's `msg!` lines.
 *
 * On failure, decodes `custom program error: 0xN` into the Rust error name.
 */
export async function sendIx(
  connection: Connection,
  rpcUrl: string,
  label: string,
  ix: TransactionInstruction,
  signers: Signer[],
): Promise<string> {
  try {
    const signature = await sendAndConfirmTransaction(connection, new Transaction().add(ix), signers, {
      commitment: "confirmed",
    });
    console.log(`  ✔ ${label}`);
    console.log(`    signature: ${signature}`);
    console.log(`    explorer:  ${explorerTxUrl(signature, rpcUrl)}`);
    await printProgramLogs(connection, signature);
    return signature;
  } catch (err) {
    const logs = err instanceof SendTransactionError ? await err.getLogs(connection) : undefined;
    const code = /custom program error: 0x([0-9a-f]+)/i.exec(String(err) + (logs ?? []).join("\n"));
    const name = code?.[1] ? programErrorName(parseInt(code[1], 16)) : undefined;
    console.error(`  ✘ ${label} failed${name ? `: CrowdfundingError::${name}` : ""}`);
    logs?.forEach((line) => console.error(`    ${line}`));
    throw err;
  }
}

async function printProgramLogs(connection: Connection, signature: string): Promise<void> {
  const tx = await connection.getTransaction(signature, {
    commitment: "confirmed",
    maxSupportedTransactionVersion: 0,
  });
  for (const line of tx?.meta?.logMessages ?? []) {
    if (line.startsWith("Program log: ")) console.log(`    log:       ${line.slice(13)}`);
  }
}

/**
 * Current cluster time: the block time of the latest confirmed slot.
 *
 * This tracks the on-chain `Clock::unix_timestamp` the program checks, which
 * can differ from the local wall clock by several seconds.
 */
export async function getClusterTime(connection: Connection): Promise<bigint> {
  for (let attempt = 0; attempt < 10; attempt++) {
    const slot = await connection.getSlot("confirmed");
    const time = await connection.getBlockTime(slot).catch(() => null);
    if (time !== null) return BigInt(time);
    await sleep(500);
  }
  throw new Error("Could not read cluster time (getBlockTime returned null)");
}

/** Blocks until cluster time is strictly past `deadline`. */
export async function waitForDeadline(connection: Connection, deadline: bigint): Promise<void> {
  process.stdout.write(`  … waiting for cluster time to pass deadline ${deadline}`);
  for (;;) {
    const now = await getClusterTime(connection);
    if (now > deadline) break;
    process.stdout.write(` (${deadline - now + 1n}s)`);
    await sleep(Math.min(Number(deadline - now + 1n) * 1000, 5000));
  }
  process.stdout.write(" ✔\n");
}

/** Fetches, decodes and prints a campaign and its vault balance. */
export async function printCampaign(
  connection: Connection,
  programId: PublicKey,
  campaign: PublicKey,
): Promise<void> {
  const account = await connection.getAccountInfo(campaign, "confirmed");
  if (!account) throw new Error(`Campaign ${campaign.toBase58()} not found`);
  if (!account.owner.equals(programId)) throw new Error("Campaign is not owned by the program");

  const state = decodeCampaign(account.data);
  const vaultBalance = await connection.getBalance(findVaultAddress(programId, campaign), "confirmed");
  const now = await getClusterTime(connection);
  console.log("    state:", {
    creator: state.creator.toBase58(),
    goal: formatSol(state.goal),
    raised: formatSol(state.raised),
    deadline: `${state.deadline} (${new Date(Number(state.deadline) * 1000).toISOString()})`,
    claimed: state.claimed,
    status: campaignStatus(state, now),
    vaultBalance: formatSol(vaultBalance),
  });
}
