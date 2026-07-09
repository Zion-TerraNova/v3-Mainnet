/**
 * deploy-defi.ts — Deploy ZIONGovernance + ZIONTreasury + ZIONStaking
 *
 * All three contracts use the already-deployed wZION token as the base asset.
 * Run this AFTER `deploy.ts` (wZION + ZIONBridge must be live).
 *
 * Usage:
 *   npx hardhat run scripts/deploy-defi.ts --network base-sepolia
 *   npx hardhat run scripts/deploy-defi.ts --network base
 *
 * Required env vars:
 *   DEPLOYER_PRIVATE_KEY  — signer (set in .env)
 *   WZION_ADDRESS         — deployed wZION address (defaults to Base Sepolia)
 *   GUARDIAN_ADDRESS      — guardian multisig (defaults to deployer on testnet)
 *   TREASURY_SIGNER2      — 2nd treasury multi-sig signer (testnet: optional)
 *
 * Optional:
 *   STAKING_APR_BPS       — initial staking APR in basis points (default 1200 = 12%)
 *   STAKING_SEED_AMOUNT   — wZION to seed into reward pool (default 0, fund later)
 */

import { ethers, network } from "hardhat";
import * as fs from "fs";
import * as path from "path";

// ─── Constants ────────────────────────────────────────────────────────────────

/** wZION address — same on Base Sepolia testnet and Base Mainnet */
const WZION_DEFAULT = "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6";

/** Default staking APR: 12% */
const DEFAULT_APR_BPS = 1200;

// ─────────────────────────────────────────────────────────────────────────────

async function main() {
  const networkName = network.name;
  const isLocal = networkName === "hardhat" || networkName === "localhost";
  const { chainId } = await ethers.provider.getNetwork();

  console.log("\n" + "═".repeat(70));
  console.log("  ZION DeFi Stack Deploy — ZIONGovernance + ZIONTreasury + ZIONStaking");
  console.log("═".repeat(70));
  console.log(`Network:  ${networkName} (chain ${chainId})`);

  const [deployer] = await ethers.getSigners();
  const balance    = await ethers.provider.getBalance(deployer.address);
  console.log(`Deployer: ${deployer.address}`);
  console.log(`Balance:  ${ethers.formatEther(balance)} ETH`);

  if (balance < ethers.parseEther("0.005")) {
    console.error("❌ Insufficient ETH balance (need ≥ 0.005 ETH)");
    console.error("   Get Base Sepolia ETH at: https://faucet.quicknode.com/base/sepolia");
    process.exit(1);
  }

  // ── Resolve addresses ─────────────────────────────────────────────────────

  const wzionAddr   = process.env.WZION_ADDRESS   || (isLocal ? "" : WZION_DEFAULT);
  const guardianAddr = process.env.GUARDIAN_ADDRESS || deployer.address;
  const aprBps      = parseInt(process.env.STAKING_APR_BPS || String(DEFAULT_APR_BPS));

  if (!wzionAddr || !ethers.isAddress(wzionAddr)) {
    console.error("❌ WZION_ADDRESS not set or invalid.");
    console.error("   Set env var WZION_ADDRESS=0x... or run deploy.ts first.");
    process.exit(1);
  }

  // Local: deploy a mock ERC-20 to use as wZION
  let finalWzion = wzionAddr;
  if (isLocal) {
    console.log("\n⚙️  Local network — deploying mock wZION...");
    const Mock = await ethers.getContractFactory("WZION");
    const mock = await Mock.deploy(deployer.address, deployer.address, deployer.address);
    await mock.waitForDeployment();
    finalWzion = await mock.getAddress();
    console.log(`   Mock wZION: ${finalWzion}`);
  }

  console.log(`\nwZION:    ${finalWzion}`);
  console.log(`Guardian: ${guardianAddr}`);
  console.log(`APR:      ${aprBps / 100}% (${aprBps} bps)`);

  // ── Nonce management (avoids "nonce too low" on fast testnets) ─────────────
  let nonce = await ethers.provider.getTransactionCount(deployer.address, "pending");
  const nextNonce = () => nonce++;

  // Helper: wait for TX to be fully confirmed + extra delay for public RPC in-flight limit
  const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));
  const waitForConfirm = async (tx: { wait: (confirms?: number) => Promise<any> }) => {
    const receipt = await tx.wait(2); // wait for 2 confirmations
    await sleep(3000); // extra delay for public RPC in-flight limit
    return receipt;
  };

  // ── Step 1: ZIONGovernance ────────────────────────────────────────────────

  console.log("\n📜 Step 1: Deploying ZIONGovernance...");
  // constructor(address _zionToken)
  const Governance = await ethers.getContractFactory("ZIONGovernance");
  const governance = await Governance.deploy(finalWzion, { nonce: nextNonce() });
  await governance.waitForDeployment();
  await sleep(3000);
  const govAddr = await governance.getAddress();
  console.log(`   ✅ ZIONGovernance: ${govAddr}`);

  // ── Step 2: ZIONTreasury ──────────────────────────────────────────────────

  console.log("\n🏛️  Step 2: Deploying ZIONTreasury...");

  // constructor(address _zionToken, address[] memory _signers, uint256 _required)
  // Note: contract requires _required >= 3.
  // Testnet: pad to 3 signers (all deployer) — replace with real addresses on mainnet!
  const treasurySigners: string[] = [deployer.address];
  for (const envKey of ["TREASURY_SIGNER2", "TREASURY_SIGNER3", "TREASURY_SIGNER4", "TREASURY_SIGNER5"]) {
    const addr = process.env[envKey];
    if (addr && ethers.isAddress(addr) && !treasurySigners.includes(addr)) {
      treasurySigners.push(addr);
    }
  }
  // Testnet: use only unique signers, threshold = 1 if only deployer available
  const isTestnet = isLocal || networkName.includes("sepolia") || networkName.includes("testnet");
  if (!isTestnet && treasurySigners.length < 3) {
    console.error("❌ ZIONTreasury requires >= 3 signers on mainnet. Set TREASURY_SIGNER2 + TREASURY_SIGNER3.");
    process.exit(1);
  }
  const uniqueSigners = [...new Set(treasurySigners)];
  const threshold = isTestnet ? 1 : Math.max(3, Math.ceil(uniqueSigners.length * 0.6));

  const Treasury  = await ethers.getContractFactory("ZIONTreasury");
  const treasury  = await Treasury.deploy(finalWzion, uniqueSigners, threshold, { nonce: nextNonce() });
  await treasury.waitForDeployment();
  await sleep(3000);
  const treasuryAddr = await treasury.getAddress();
  console.log(`   ✅ ZIONTreasury: ${treasuryAddr}`);
  console.log(`      Signers: ${uniqueSigners.join(", ")}`);
  console.log(`      Threshold: ${threshold}-of-${uniqueSigners.length}`);

  // ── Step 3: ZIONStaking ───────────────────────────────────────────────────

  console.log("\n🏗️  Step 3: Deploying ZIONStaking...");
  const Staking = await ethers.getContractFactory("ZIONStaking");
  const staking = await Staking.deploy(
    finalWzion,
    deployer.address,   // admin
    guardianAddr,       // guardian
    aprBps,
    { nonce: nextNonce() }
  );
  await staking.waitForDeployment();
  await sleep(3000);
  const stakingAddr = await staking.getAddress();
  console.log(`   ✅ ZIONStaking: ${stakingAddr}`);
  console.log(`      APR: ${aprBps / 100}% | Cooldown: 7 days`);

  // Grant REWARD_FUNDER_ROLE to treasury so it can fund the staking pool
  console.log("\n🔐 Step 4: Grant REWARD_FUNDER_ROLE to ZIONTreasury...");
  const REWARD_FUNDER_ROLE = await staking.REWARD_FUNDER_ROLE();
  const grantTx = await staking.grantRole(REWARD_FUNDER_ROLE, treasuryAddr, { nonce: nextNonce() });
  await grantTx.wait(2);
  await sleep(3000);
  console.log(`   ✅ REWARD_FUNDER_ROLE granted to treasury`);

  // Grant staking awareness to governance (read-only — just logging for config)
  console.log("\n📋 Step 5: Configure governance ↔ staking link...");
  // ZIONGovernance reads votingWeight from staking contract via interface.
  // We set the staking contract address in governance if the function exists.
  try {
    const govContract = governance as unknown as {
      setStakingContract?: (addr: string) => Promise<{ wait: () => Promise<void> }>;
    };
    if (typeof govContract.setStakingContract === "function") {
      const stakeTx = await govContract.setStakingContract(stakingAddr, { nonce: nextNonce() } as never);
      await (stakeTx as unknown as { wait: () => Promise<void> }).wait();
      console.log(`   ✅ Governance staking contract set to ${stakingAddr}`);
    } else {
      console.log(`   ℹ️  ZIONGovernance.setStakingContract() not implemented — skipped`);
    }
  } catch (e) {
    console.log(`   ℹ️  Could not set staking in governance: ${(e as Error).message}`);
  }

  // ── Seed staking reward pool (optional) ───────────────────────────────────

  const seedAmount = process.env.STAKING_SEED_AMOUNT
    ? BigInt(process.env.STAKING_SEED_AMOUNT)
    : 0n;

  if (seedAmount > 0n && isLocal) {
    console.log(`\n💰 Seeding staking reward pool with ${ethers.formatUnits(seedAmount, 8)} wZION...`);
    const wzion   = await ethers.getContractAt("WZION", finalWzion);
    const approveTx = await wzion.approve(stakingAddr, seedAmount);
    await approveTx.wait();
    const fundTx  = await staking.fundRewardPool(seedAmount);
    await fundTx.wait();
    console.log(`   ✅ Reward pool funded`);
  }

  // ── Save deployment JSON ──────────────────────────────────────────────────

  const deployedAt = new Date().toISOString();
  const output = {
    network: networkName,
    chainId: Number(chainId),
    wzion:       finalWzion,
    governance:  govAddr,
    treasury:    treasuryAddr,
    staking:     stakingAddr,
    config: {
      stakingAprBps:    aprBps,
      cooldownSeconds:  7 * 24 * 3600,
      treasurySigners,
      treasuryThreshold: threshold,
      guardian:          guardianAddr,
    },
    deployedAt,
  };

  const outPath = path.join(__dirname, "..", "deployed-defi.json");
  fs.writeFileSync(outPath, JSON.stringify(output, null, 2));
  console.log(`\n📁 Deployment saved to: deployed-defi.json`);

  // ── Summary ───────────────────────────────────────────────────────────────

  console.log("\n" + "═".repeat(70));
  console.log("  ZION DeFi Deploy Summary");
  console.log("═".repeat(70));
  console.log(`  Network:       ${networkName} (${chainId})`);
  console.log(`  wZION:         ${finalWzion}`);
  console.log(`  ZIONGovernance: ${govAddr}`);
  console.log(`  ZIONTreasury:   ${treasuryAddr}`);
  console.log(`  ZIONStaking:    ${stakingAddr}`);
  console.log(`  Staking APR:    ${aprBps / 100}%`);
  console.log("═".repeat(70));

  if (!isLocal) {
    console.log("\n⚠️  Next steps:");
    console.log(`  1. Fund staking reward pool:`);
    console.log(`     staking.fundRewardPool(<amount wZION>)`);
    console.log(`  2. Set pool address in .env:`);
    console.log(`     GOVERNANCE_ADDRESS=${govAddr}`);
    console.log(`     TREASURY_ADDRESS=${treasuryAddr}`);
    console.log(`     STAKING_ADDRESS=${stakingAddr}`);
    console.log(`  3. Run optional: npx hardhat run scripts/verify.ts --network ${networkName}`);
    console.log(`  4. Deploy Uniswap V3 pool:`);
    console.log(`     WZION_ADDRESS_SEPOLIA=${finalWzion} npx hardhat run scripts/deploy-pool.ts --network ${networkName}`);
  }
}

main()
  .then(() => process.exit(0))
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
