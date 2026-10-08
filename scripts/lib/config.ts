/**
 * Runtime configuration for the demo: program id, RPC endpoint, payer
 * keypair and Explorer links.
 *
 * The payer keypair is only *read at runtime* from the local filesystem; it is
 * never logged, copied or committed.
 */
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { Keypair, PublicKey } from "@solana/web3.js";

export const DEFAULT_RPC_URL = "https://api.devnet.solana.com";

export interface DemoConfig {
  programId: PublicKey;
  rpcUrl: string;
  payer: Keypair;
  keypairPath: string;
  /** Seconds between campaign creation and its deadline. */
  deadlineSeconds: number;
}

function expandHome(path: string): string {
  return path.startsWith("~/") ? join(homedir(), path.slice(2)) : path;
}

/**
 * Resolves the payer keypair path:
 * `KEYPAIR` env → `keypair_path` in the Solana CLI config → `~/.config/solana/id.json`.
 */
export function resolveKeypairPath(): string {
  if (process.env.KEYPAIR) return expandHome(process.env.KEYPAIR);

  const cliConfig = join(homedir(), ".config", "solana", "cli", "config.yml");
  if (existsSync(cliConfig)) {
    // Single flat key; a regex avoids pulling in a YAML parser.
    const match = readFileSync(cliConfig, "utf8").match(/^keypair_path:\s*['"]?(.+?)['"]?\s*$/m);
    if (match?.[1]) return expandHome(match[1]);
  }
  return join(homedir(), ".config", "solana", "id.json");
}

function loadKeypair(path: string): Keypair {
  if (!existsSync(path)) {
    throw new Error(
      `Keypair file not found at ${path}. Set KEYPAIR=/path/to/keypair.json ` +
        "or create one with `solana-keygen new`.",
    );
  }
  const secret = Uint8Array.from(JSON.parse(readFileSync(path, "utf8")) as number[]);
  return Keypair.fromSecretKey(secret);
}

function parseProgramId(raw: string | undefined): PublicKey {
  if (!raw) {
    throw new Error(
      "Missing program id. Usage: PROGRAM_ID=<address> npm run demo  (or: npm run demo -- <address>)",
    );
  }
  try {
    return new PublicKey(raw);
  } catch {
    throw new Error(`PROGRAM_ID is not a valid base58 public key: ${raw}`);
  }
}

/** Builds the demo configuration from CLI args and environment variables. */
export function loadConfig(argv: string[] = process.argv.slice(2)): DemoConfig {
  const programId = parseProgramId(argv[0] ?? process.env.PROGRAM_ID);
  const rpcUrl = process.env.RPC_URL ?? DEFAULT_RPC_URL;
  const keypairPath = resolveKeypairPath();
  const deadlineSeconds = Number(process.env.DEADLINE_SECONDS ?? 20);
  if (!Number.isInteger(deadlineSeconds) || deadlineSeconds <= 0) {
    throw new Error("DEADLINE_SECONDS must be a positive integer");
  }
  return { programId, rpcUrl, payer: loadKeypair(keypairPath), keypairPath, deadlineSeconds };
}

/**
 * Explorer `cluster` query string for an RPC URL.
 *
 * Any Devnet endpoint whose URL mentions "devnet" maps to `?cluster=devnet`;
 * unknown endpoints use Explorer's custom-RPC mode. Override with
 * `EXPLORER_CLUSTER=devnet|testnet|mainnet-beta` if detection is wrong.
 */
export function explorerClusterParam(rpcUrl: string): string {
  const override = process.env.EXPLORER_CLUSTER;
  const cluster =
    override ??
    (/devnet/i.test(rpcUrl)
      ? "devnet"
      : /testnet/i.test(rpcUrl)
        ? "testnet"
        : /mainnet/i.test(rpcUrl)
          ? "mainnet-beta"
          : "custom");
  if (cluster === "mainnet-beta") return "";
  if (cluster === "custom") return `?cluster=custom&customUrl=${encodeURIComponent(rpcUrl)}`;
  return `?cluster=${cluster}`;
}

export function explorerTxUrl(signature: string, rpcUrl: string): string {
  return `https://explorer.solana.com/tx/${signature}${explorerClusterParam(rpcUrl)}`;
}

export function explorerAddressUrl(address: PublicKey, rpcUrl: string): string {
  return `https://explorer.solana.com/address/${address.toBase58()}${explorerClusterParam(rpcUrl)}`;
}

/** Strips query strings (often API keys) from an RPC URL before printing it. */
export function redactRpcUrl(rpcUrl: string): string {
  try {
    const url = new URL(rpcUrl);
    return url.search ? `${url.origin}${url.pathname} (query redacted)` : url.toString();
  } catch {
    return rpcUrl;
  }
}
