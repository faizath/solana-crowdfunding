/**
 * End-to-end demo of the crowdfunding program on Solana Devnet.
 *
 * Runs both campaign outcomes with the payer acting as creator *and* donor:
 *
 *   A. success path: create → contribute ≥ goal → wait for deadline → withdraw
 *   B. refund path:  create → contribute < goal → wait for deadline → refund
 *
 * Usage (from `scripts/`):
 *
 *   npm install
 *   PROGRAM_ID=<deployed program id> npm run demo
 *   npm run demo -- <deployed program id>
 *
 * Environment:
 *   PROGRAM_ID        program address (or pass as first CLI argument)
 *   RPC_URL           default https://api.devnet.solana.com (any Devnet RPC works)
 *   KEYPAIR           payer keypair path; default = `keypair_path` from
 *                     ~/.config/solana/cli/config.yml, else ~/.config/solana/id.json
 *   DEADLINE_SECONDS  campaign duration, default 20
 *   EXPLORER_CLUSTER  force the Explorer cluster (devnet|testnet|mainnet-beta)
 *
 * Total cost is roughly 0.02 SOL of Devnet SOL (most of it returned at the end).
 */
import { Connection, Keypair, LAMPORTS_PER_SOL, PublicKey } from "@solana/web3.js";
import {
  formatSol,
  getClusterTime,
  printCampaign,
  sendIx,
  waitForDeadline,
} from "./lib/cluster.js";
import {
  explorerAddressUrl,
  explorerTxUrl,
  loadConfig,
  redactRpcUrl,
  type DemoConfig,
} from "./lib/config.js";
import {
  contributeIx,
  createCampaignIx,
  findContributionAddress,
  findVaultAddress,
  refundIx,
  withdrawIx,
} from "./lib/program.js";

const sol = (amount: number): bigint => BigInt(Math.round(amount * LAMPORTS_PER_SOL));

/** Goal / contribution sizes (lamports). Kept tiny so the faucet suffices. */
const CAMPAIGN_A = { goal: sol(0.01), contribution: sol(0.01) };
const CAMPAIGN_B = { goal: sol(0.05), contribution: sol(0.005) };
/** Contributions + rent for two campaigns, two vault reserves, two records, fees. */
const MIN_BALANCE = sol(0.03);

interface Step {
  step: string;
  signature: string;
}

async function createCampaign(
  connection: Connection,
  config: DemoConfig,
  goal: bigint,
): Promise<{ campaign: PublicKey; deadline: bigint; signature: string }> {
  const { programId, payer, rpcUrl } = config;
  const campaign = Keypair.generate();
  const deadline = (await getClusterTime(connection)) + BigInt(config.deadlineSeconds);

  console.log(`    campaign:  ${campaign.publicKey.toBase58()}`);
  console.log(`               ${explorerAddressUrl(campaign.publicKey, rpcUrl)}`);
  console.log(`    vault:     ${findVaultAddress(programId, campaign.publicKey).toBase58()}`);

  const signature = await sendIx(
    connection,
    rpcUrl,
    `CreateCampaign(goal=${formatSol(goal)}, deadline=${deadline})`,
    createCampaignIx(programId, payer.publicKey, campaign.publicKey, goal, deadline),
    [payer, campaign],
  );
  await printCampaign(connection, programId, campaign.publicKey);
  return { campaign: campaign.publicKey, deadline, signature };
}

async function contribute(
  connection: Connection,
  config: DemoConfig,
  campaign: PublicKey,
  amount: bigint,
): Promise<string> {
  const { programId, payer, rpcUrl } = config;
  console.log(
    `    contribution PDA: ${findContributionAddress(programId, campaign, payer.publicKey).toBase58()}`,
  );
  const signature = await sendIx(
    connection,
    rpcUrl,
    `Contribute(${formatSol(amount)})`,
    contributeIx(programId, payer.publicKey, campaign, amount),
    [payer],
  );
  await printCampaign(connection, programId, campaign);
  return signature;
}

async function runSuccessPath(connection: Connection, config: DemoConfig): Promise<Step[]> {
  const { programId, payer, rpcUrl } = config;
  console.log("\n━━ Campaign A: goal reached → creator withdraws ━━");
  const { campaign, deadline, signature: create } = await createCampaign(
    connection,
    config,
    CAMPAIGN_A.goal,
  );
  const contributed = await contribute(connection, config, campaign, CAMPAIGN_A.contribution);

  await waitForDeadline(connection, deadline);
  const withdrawn = await sendIx(
    connection,
    rpcUrl,
    "Withdraw",
    withdrawIx(programId, payer.publicKey, campaign),
    [payer],
  );
  await printCampaign(connection, programId, campaign);

  return [
    { step: "Create campaign A", signature: create },
    { step: "Contribute (A)", signature: contributed },
    { step: "Withdraw (A)", signature: withdrawn },
  ];
}

async function runRefundPath(connection: Connection, config: DemoConfig): Promise<Step[]> {
  const { programId, payer, rpcUrl } = config;
  console.log("\n━━ Campaign B: goal missed → donor refunds ━━");
  const { campaign, deadline, signature: create } = await createCampaign(
    connection,
    config,
    CAMPAIGN_B.goal,
  );
  const contributed = await contribute(connection, config, campaign, CAMPAIGN_B.contribution);

  await waitForDeadline(connection, deadline);
  const refunded = await sendIx(
    connection,
    rpcUrl,
    "Refund",
    refundIx(programId, payer.publicKey, campaign),
    [payer],
  );
  await printCampaign(connection, programId, campaign);

  return [
    { step: "Create campaign B", signature: create },
    { step: "Contribute (B)", signature: contributed },
    { step: "Refund (B)", signature: refunded },
  ];
}

async function main(): Promise<void> {
  const config = loadConfig();
  const connection = new Connection(config.rpcUrl, "confirmed");

  const program = await connection.getAccountInfo(config.programId);
  if (!program?.executable) {
    throw new Error(
      `No executable program at ${config.programId.toBase58()} on ${redactRpcUrl(config.rpcUrl)}`,
    );
  }
  const balance = BigInt(await connection.getBalance(config.payer.publicKey));

  console.log("Solana Crowdfunding demo");
  console.log(`  rpc:     ${redactRpcUrl(config.rpcUrl)}`);
  console.log(`  program: ${config.programId.toBase58()}`);
  console.log(`  payer:   ${config.payer.publicKey.toBase58()} (${formatSol(balance)})`);
  if (balance < MIN_BALANCE) {
    throw new Error(
      `Payer needs at least ${formatSol(MIN_BALANCE)}. Get Devnet SOL at https://faucet.solana.com`,
    );
  }

  const steps = [
    ...(await runSuccessPath(connection, config)),
    ...(await runRefundPath(connection, config)),
  ];

  console.log("\n━━ Summary (paste into the README's Deployment section) ━━");
  console.log("| Step | Signature | Explorer |");
  console.log("|------|-----------|----------|");
  for (const { step, signature } of steps) {
    console.log(`| ${step} | \`${signature}\` | [view](${explorerTxUrl(signature, config.rpcUrl)}) |`);
  }
  const spent = balance - BigInt(await connection.getBalance(config.payer.publicKey));
  console.log(`\nNet cost to payer: ${formatSol(spent)}`);
}

main().catch((err: unknown) => {
  console.error(`\nDemo failed: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
});
