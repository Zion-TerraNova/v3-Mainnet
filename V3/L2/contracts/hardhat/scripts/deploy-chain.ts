/**
 * deploy-chain.ts — Deploy wZION + ZIONBridge on ANY EVM mainnet chain
 *
 * Generic multi-chain deploy script. Works for Arbitrum, BSC, Polygon,
 * Optimism, Avalanche, and any future EVM chain added to hardhat.config.ts.
 *
 * Usage:
 *   npx hardhat run scripts/deploy-chain.ts --network arbitrum
 *   npx hardhat run scripts/deploy-chain.ts --network bsc
 *   npx hardhat run scripts/deploy-chain.ts --network polygon
 *   npx hardhat run scripts/deploy-chain.ts --network optimism
 *   npx hardhat run scripts/deploy-chain.ts --network avalanche
 *
 * Required env vars:
 *   DEPLOYER_PRIVATE_KEY  — funded deployer (needs native gas token on target chain)
 *
 * Optional (defaults to Base mainnet validator set — same across all chains):
 *   VALIDATOR_1..5        — 5 validator addresses (default: same as Base)
 *   BRIDGE_THRESHOLD      — validator threshold (default: 5)
 *   GUARDIAN_ADDRESS      — guardian address (default: deployer)
 *
 * After deploy:
 *   1. Update bridge-mainnet.toml — target chain section with real addresses
 *   2. Update LiFiWidget.tsx — wZION address for target chain
 *   3. Update bridge-api.ts — target chain entry
 *   4. Restart zion-edge-bridge.service
 *   5. E2E test: lock ZION on L1 → mint wZION on target chain
 */

import { ethers, network } from "hardhat";
import * as fs from "fs";
import * as path from "path";

// ─── Chain metadata ──────────────────────────────────────────────────────────

interface ChainMeta {
  name: string;
  nativeSymbol: string;
  minGasBalance: string;      // minimum native token needed for deploy
  blockTimeMs: number;        // extra delay between TXs for RPC propagation
  explorerUrl: (addr: string) => string;
  finalityBlocks: number;     // for bridge-mainnet.toml
}

const CHAIN_META: Record<string, ChainMeta> = {
  arbitrum: {
    name: "Arbitrum One",
    nativeSymbol: "ETH",
    minGasBalance: "0.005",
    blockTimeMs: 5000,
    explorerUrl: (a) => `https://arbiscan.io/address/${a}`,
    finalityBlocks: 10,
  },
  bsc: {
    name: "BNB Smart Chain",
    nativeSymbol: "BNB",
    minGasBalance: "0.01",
    blockTimeMs: 3000,
    explorerUrl: (a) => `https://bscscan.com/address/${a}`,
    finalityBlocks: 15,
  },
  polygon: {
    name: "Polygon PoS",
    nativeSymbol: "POL",
    minGasBalance: "0.05",
    blockTimeMs: 3000,
    explorerUrl: (a) => `https://polygonscan.com/address/${a}`,
    finalityBlocks: 128,
  },
  optimism: {
    name: "Optimism",
    nativeSymbol: "ETH",
    minGasBalance: "0.005",
    blockTimeMs: 4000,
    explorerUrl: (a) => `https://optimistic.etherscan.io/address/${a}`,
    finalityBlocks: 10,
  },
  avalanche: {
    name: "Avalanche C-Chain",
    nativeSymbol: "AVAX",
    minGasBalance: "0.1",
    blockTimeMs: 3000,
    explorerUrl: (a) => `https://snowtrace.io/address/${a}`,
    finalityBlocks: 12,
  },
};

// ─── Validator defaults (same as Base mainnet — EVM address is deterministic) ─

const DEFAULT_VALIDATORS = [
  "0x9b5b9a6c4ce4bcd4479d8ea6d12cd7bfeb61085f", // validator-1 (new — hard reset 2026-07-06)
  "0x8a804afd4c200e95f415df6907da111a0258a578", // validator-2 (new — hard reset 2026-07-06)
  "0x694f3b43f4bf77dfbef53224791272d102449218", // validator-3 (new — hard reset 2026-07-06)
  "0x64c85af40143484c12316723192a0d71c10e82b8", // validator-4 (new — hard reset 2026-07-06)
  "0xe093ff26da65079df435a89834497abc380b59ae", // validator-5 (new — hard reset 2026-07-06)
];

const DEFAULT_THRESHOLD = 5;

// ─────────────────────────────────────────────────────────────────────────────

async function main() {
  const networkName = network.name;
  const { chainId } = await ethers.provider.getNetwork();

  console.log("\n" + "═".repeat(70));
  console.log("  ZION Bridge Deploy — wZION + ZIONBridge");
  console.log("═".repeat(70));

  const meta = CHAIN_META[networkName];
  if (!meta) {
    console.error(`❌ This script supports: ${Object.keys(CHAIN_META).join(", ")}`);
    console.error(`   Got network: "${networkName}" — add it to CHAIN_META in this script`);
    process.exit(1);
  }

  console.log(`Network:      ${networkName} (chain ${chainId})`);
  console.log(`Chain name:   ${meta.name}`);
  console.log(`Native token: ${meta.nativeSymbol}`);
  console.log(`Finality:     ${meta.finalityBlocks} blocks`);

  const [deployer] = await ethers.getSigners();
  const balance = await ethers.provider.getBalance(deployer.address);
  console.log(`Deployer:     ${deployer.address}`);
  console.log(`Balance:      ${ethers.formatEther(balance)} ${meta.nativeSymbol}`);

  const minBalance = ethers.parseEther(meta.minGasBalance);
  if (balance < minBalance) {
    console.error(`❌ Insufficient ${meta.nativeSymbol} balance (need ≥ ${meta.minGasBalance} ${meta.nativeSymbol} for gas)`);
    console.error(`   Fund the deployer address on ${meta.name}:`);
    console.error(`   ${meta.explorerUrl(deployer.address)}`);
    process.exit(1);
  }

  // ── Resolve validator set ──────────────────────────────────────────────────

  const validators: string[] = [];
  for (let i = 1; i <= 5; i++) {
    const addr = process.env[`VALIDATOR_${i}`] || DEFAULT_VALIDATORS[i - 1];
    if (!ethers.isAddress(addr)) {
      console.error(`❌ Invalid validator ${i} address: ${addr}`);
      process.exit(1);
    }
    validators.push(addr);
  }
  const threshold = parseInt(process.env.BRIDGE_THRESHOLD || String(DEFAULT_THRESHOLD));
  const guardianAddr = process.env.GUARDIAN_ADDRESS || deployer.address;

  console.log(`\nValidators (${threshold}-of-${validators.length}):`);
  validators.forEach((v, i) => console.log(`  ${i + 1}. ${v}`));
  console.log(`Guardian: ${guardianAddr}`);

  // ── Nonce management ───────────────────────────────────────────────────────

  let nonce = await ethers.provider.getTransactionCount(deployer.address, "pending");
  const nextNonce = () => nonce++;
  const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));
  const waitForConfirm = async (tx: { wait: (confirms?: number) => Promise<any> }) => {
    const receipt = await tx.wait(2);
    await sleep(meta.blockTimeMs);
    return receipt;
  };

  // ── Step 1: Deploy wZION ───────────────────────────────────────────────────

  console.log("\n📜 Step 1: Deploying wZION (ERC-20)...");
  const WZION = await ethers.getContractFactory("WZION");
  // constructor(admin, bridge, guardian) — bridge is placeholder (deployer),
  // we'll grant BRIDGE_ROLE to ZIONBridge after it's deployed
  const wzion = await WZION.deploy(deployer.address, deployer.address, guardianAddr, {
    nonce: nextNonce(),
  });
  await wzion.waitForDeployment();
  await sleep(meta.blockTimeMs);
  const wzionAddr = await wzion.getAddress();
  console.log(`   ✅ wZION: ${wzionAddr}`);
  console.log(`      Name: Wrapped ZION | Symbol: wZION | Decimals: 18`);
  console.log(`      MAX_SUPPLY: 144,000,000,000 wZION`);
  console.log(`      Explorer: ${meta.explorerUrl(wzionAddr)}`);

  // ── Step 2: Deploy ZIONBridge ──────────────────────────────────────────────

  console.log("\n🌉 Step 2: Deploying ZIONBridge (5/5 multisig)...");
  const ZIONBridge = await ethers.getContractFactory("ZIONBridge");
  // constructor(admin, guardian, wZIONAddr, validators[], threshold)
  const bridge = await ZIONBridge.deploy(
    deployer.address,    // admin
    guardianAddr,        // guardian
    wzionAddr,           // wZION
    validators,          // 5 validator addresses
    threshold,           // 5
    { nonce: nextNonce() }
  );
  await bridge.waitForDeployment();
  await sleep(meta.blockTimeMs);
  const bridgeAddr = await bridge.getAddress();
  console.log(`   ✅ ZIONBridge: ${bridgeAddr}`);
  console.log(`      Validators: ${validators.length} | Threshold: ${threshold}`);
  console.log(`      Explorer: ${meta.explorerUrl(bridgeAddr)}`);

  // ── Step 3: Grant BRIDGE_ROLE on wZION for ZIONBridge ──────────────────────

  console.log("\n🔐 Step 3: Granting BRIDGE_ROLE on wZION for ZIONBridge...");
  const BRIDGE_ROLE = await wzion.BRIDGE_ROLE();
  const grantTx = await wzion.grantRole(BRIDGE_ROLE, bridgeAddr, { nonce: nextNonce() });
  await waitForConfirm(grantTx);
  console.log(`   ✅ BRIDGE_ROLE granted to ZIONBridge (${bridgeAddr})`);

  // ── Step 4: Renounce deployer's temporary BRIDGE_ROLE (security) ───────────

  console.log("\n🔒 Step 4: Renouncing deployer's temporary BRIDGE_ROLE...");
  try {
    const renounceTx = await wzion.renounceRole(BRIDGE_ROLE, deployer.address, {
      nonce: nextNonce(),
    });
    await waitForConfirm(renounceTx);
    console.log(`   ✅ Deployer BRIDGE_ROLE renounced — only ZIONBridge can mint/burn`);
  } catch (e) {
    console.log(`   ⚠️  Could not renounce (may need DEFAULT_ADMIN): ${(e as Error).message}`);
  }

  // ── Save deployment JSON ───────────────────────────────────────────────────

  const deployedAt = new Date().toISOString();
  const output = {
    network: networkName,
    chainId: Number(chainId),
    chainName: meta.name,
    wzion: wzionAddr,
    bridge: bridgeAddr,
    config: {
      validators,
      threshold,
      guardian: guardianAddr,
      deployer: deployer.address,
      finalityBlocks: meta.finalityBlocks,
    },
    deployedAt,
  };

  const outPath = path.join(__dirname, "..", `deployed-${networkName}.json`);
  fs.writeFileSync(outPath, JSON.stringify(output, null, 2));
  console.log(`\n📁 Deployment saved to: deployed-${networkName}.json`);

  // ── Summary ────────────────────────────────────────────────────────────────

  console.log("\n" + "═".repeat(70));
  console.log(`  ZION ${meta.name} Deploy Summary`);
  console.log("═".repeat(70));
  console.log(`  Network:       ${networkName} (${chainId})`);
  console.log(`  wZION:         ${wzionAddr}`);
  console.log(`  ZIONBridge:    ${bridgeAddr}`);
  console.log(`  Validators:    ${threshold}-of-${validators.length}`);
  console.log(`  Finality:      ${meta.finalityBlocks} blocks`);
  console.log("═".repeat(70));

  console.log("\n⚠️  Next steps:");
  console.log(`  1. Update V3/L2/bridge/config/bridge-mainnet.toml:`);
  console.log(`     - Set wzion_address = "${wzionAddr}"`);
  console.log(`     - Set bridge_contract_address = "${bridgeAddr}"`);
  console.log(`     - Set finality_blocks = ${meta.finalityBlocks}`);
  console.log(`     - Set enabled = true`);
  console.log(`  2. Update LiFiWidget.tsx — wZION ${meta.name} address`);
  console.log(`  3. Update bridge-api.ts — ${networkName} chain entry`);
  console.log(`  4. Restart zion-edge-bridge.service on Edge`);
  console.log(`  5. E2E test: lock ZION on L1 → mint wZION on ${meta.name}`);
  console.log(`  6. Verify on explorer: ${meta.explorerUrl(wzionAddr)}`);
}

main()
  .then(() => process.exit(0))
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
