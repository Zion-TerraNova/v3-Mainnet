/**
 * F-04 — Deploy ZIONFarm (Yield Farming)
 *
 * Usage:
 *   npx hardhat run scripts/deploy-farm.ts --network base-sepolia
 *   npx hardhat run scripts/deploy-farm.ts --network base
 *
 * Env vars:
 *   DEPLOYER_PRIVATE_KEY   — signer (set in .env)
 *   WZION_ADDRESS          — wZION reward token address (overrides auto-detect)
 *   FARM_ADMIN             — admin (defaults to deployer)
 *   FARM_GUARDIAN          — guardian (defaults to deployer)
 *   FARM_REWARD_PER_SEC    — wZION wei/second (default: 3 wZION/s = "3000000000000000000")
 *   FARM_HALVING_INTERVAL  — halving interval in seconds (default: 7776000 = 90 days)
 *
 * What this script does:
 *   1. Deploy ZIONFarm
 *   2. Add Pool 0: wZION single-asset staking (100 alloc pts)
 *   3. Add Pool 1: wZION/WETH LP placeholder (200 alloc pts) — LP token TBD after pool deploy
 *   4. Grant REWARD_FUNDER_ROLE to deployer (so seed rewards can be added)
 *   5. Save deployed-farm-{network}.json
 */

import { ethers, network, run } from "hardhat";
import * as fs from "fs";

// Known wZION addresses per network
const WZION_ADDRESSES: Record<string, string> = {
  "base-sepolia": "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6",
  "base":         "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6", // Base Mainnet — deployed 2026-06-24
};

async function main() {
  const networkName = network.name;
  console.log(`\n🌾 Deploy ZIONFarm — network: ${networkName}`);

  const [deployer] = await ethers.getSigners();
  console.log(`Deployer: ${deployer.address}`);

  // ── Resolve parameters ────────────────────────────────────────────────────
  const wzionAddr = process.env.WZION_ADDRESS
    || WZION_ADDRESSES[networkName]
    || "";

  if (!wzionAddr || wzionAddr === "") {
    throw new Error(`wZION address not set for network '${networkName}'. Set WZION_ADDRESS env var.`);
  }

  const adminAddr    = process.env.FARM_ADMIN    || deployer.address;
  const guardianAddr = process.env.FARM_GUARDIAN || deployer.address;

  const rewardPerSecond  = BigInt(process.env.FARM_REWARD_PER_SEC || ethers.parseEther("3").toString());
  const halvingInterval  = parseInt(process.env.FARM_HALVING_INTERVAL || String(90 * 24 * 3600));

  console.log(`wZION:          ${wzionAddr}`);
  console.log(`Admin:          ${adminAddr}`);
  console.log(`Guardian:       ${guardianAddr}`);
  console.log(`Reward/sec:     ${ethers.formatEther(rewardPerSecond)} wZION/s`);
  console.log(`Halving:        every ${halvingInterval / 86400} days`);

  // ── Deploy ZIONFarm ───────────────────────────────────────────────────────
  const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));
  const Factory = await ethers.getContractFactory("ZIONFarm");
  console.log("\n⏳ Deploying ZIONFarm...");
  const farm = await Factory.deploy(
    wzionAddr, adminAddr, guardianAddr, rewardPerSecond, halvingInterval
  );
  await farm.waitForDeployment();
  await sleep(3000);
  const farmAddr = await farm.getAddress();
  console.log(`✅ ZIONFarm deployed: ${farmAddr}`);

  // ── Add initial pools ─────────────────────────────────────────────────────
  console.log("\n📦 Adding initial pools...");

  // Pool 0: wZION single-asset staking (100 alloc pts)
  const tx0 = await farm.addPool(100, wzionAddr, "wZION Single", false);
  await tx0.wait(2);
  await sleep(3000);
  console.log("  Pool 0: wZION single-asset staking (100 alloc pts)");

  // Note: Pool 1 (LP token) will be added after Uniswap V3 pool is deployed
  // via: farm.addPool(200, <uniswap_lp_addr>, "wZION/WETH Uni V3", true)

  // ── Grant REWARD_FUNDER_ROLE to deployer ──────────────────────────────────
  const REWARD_FUNDER_ROLE = await farm.REWARD_FUNDER_ROLE();
  const txR = await farm.grantRole(REWARD_FUNDER_ROLE, deployer.address);
  await txR.wait(2);
  await sleep(3000);
  console.log(`✅ REWARD_FUNDER_ROLE granted to deployer`);

  // ── Save ─────────────────────────────────────────────────────────────────
  const deployInfo = {
    network:           networkName,
    deployedAt:        new Date().toISOString(),
    deployer:          deployer.address,
    ZIONFarm:          farmAddr,
    rewardToken:       wzionAddr,
    rewardPerSecond:   rewardPerSecond.toString(),
    halvingInterval,
    pools: [
      { pid: 0, name: "wZION Single", lpToken: wzionAddr, allocPoints: 100 },
    ],
    nextStep: "After Uniswap V3 pool deploy: call farm.addPool(200, <poolAddr>, 'wZION/WETH Uni V3', true)",
  };

  const outPath = `deployed-farm-${networkName}.json`;
  fs.writeFileSync(outPath, JSON.stringify(deployInfo, null, 2));
  console.log(`\n📄 Deployment saved to ${outPath}`);

  // ── Verify ────────────────────────────────────────────────────────────────
  if (networkName !== "hardhat" && networkName !== "localhost") {
    console.log("\n⏳ Waiting 10s for indexing...");
    await new Promise(r => setTimeout(r, 10_000));
    try {
      await run("verify:verify", {
        address:              farmAddr,
        constructorArguments: [wzionAddr, adminAddr, guardianAddr, rewardPerSecond, halvingInterval],
      });
      console.log("✅ Verified on block explorer");
    } catch (e: any) {
      console.warn("⚠️  Verification failed:", e.message);
    }
  }

  // ── Summary ───────────────────────────────────────────────────────────────
  console.log("\n╔══════════════════════════════════════════════════════════════╗");
  console.log("║  ZIONFarm Deployment Summary                                ║");
  console.log("╠══════════════════════════════════════════════════════════════╣");
  console.log(`║  Network:         ${networkName.padEnd(44)}║`);
  console.log(`║  ZIONFarm:        ${farmAddr.padEnd(44)}║`);
  console.log(`║  Reward token:    ${wzionAddr.padEnd(44)}║`);
  console.log(`║  Pools:           1 (wZION single)                          ║`);
  console.log("╚══════════════════════════════════════════════════════════════╝");
  console.log("\n📌 Next steps:");
  console.log("  1. Fund reward pool: npx hardhat run scripts/fund-farm.ts --network base-sepolia");
  console.log("  2. After Uni V3 pool deploy, add LP pool:");
  console.log(`     farm.addPool(200, <LP_ADDR>, 'wZION/WETH Uni V3', true)`);
}

main().catch(e => { console.error(e); process.exit(1); });
