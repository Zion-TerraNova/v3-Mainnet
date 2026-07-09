/**
 * Fund ZIONStaking reward pool with wZION
 *
 * Usage:
 *   npx hardhat run scripts/fund-staking.ts --network base
 *
 * Env vars:
 *   STAKING_ADDRESS  — deployed ZIONStaking address (overrides deployed-defi.json)
 *   WZION_ADDRESS    — wZION token address (default: Base Mainnet)
 *   STAKING_SEED_AMOUNT — wZION to seed (default: 100000 = 100K wZION)
 */
import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

const WZION_DEFAULT = "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6";
const DEFAULT_SEED = "100000"; // 100K wZION

async function main() {
  const [deployer] = await ethers.getSigners();
  console.log(`\n💰 Fund ZIONStaking — deployer: ${deployer.address}`);

  // Resolve staking address from env or deployed-defi.json
  let stakingAddr = process.env.STAKING_ADDRESS || "";
  if (!stakingAddr) {
    const deployedPath = path.join(__dirname, "..", "deployed-defi.json");
    if (fs.existsSync(deployedPath)) {
      const deployed = JSON.parse(fs.readFileSync(deployedPath, "utf8"));
      stakingAddr = deployed.staking || deployed.ZIONStaking || "";
    }
  }
  if (!stakingAddr || !ethers.isAddress(stakingAddr)) {
    throw new Error(
      "STAKING_ADDRESS not set. Either:\n" +
      "  1. Set STAKING_ADDRESS env var, or\n" +
      "  2. Run deploy-defi.ts first to generate deployed-defi.json"
    );
  }

  const wzionAddr = process.env.WZION_ADDRESS || WZION_DEFAULT;
  const seedAmount = ethers.parseEther(process.env.STAKING_SEED_AMOUNT || DEFAULT_SEED);

  console.log(`Staking:  ${stakingAddr}`);
  console.log(`wZION:    ${wzionAddr}`);
  console.log(`Seed:     ${ethers.formatEther(seedAmount)} wZION`);

  const wzion   = await ethers.getContractAt("WZION", wzionAddr);
  const staking = await ethers.getContractAt("ZIONStaking", stakingAddr);

  const bal = await wzion.balanceOf(deployer.address);
  console.log(`Deployer wZION balance: ${ethers.formatEther(bal)}`);
  if (bal < seedAmount) {
    throw new Error(`Insufficient wZION (have ${ethers.formatEther(bal)}, need ${ethers.formatEther(seedAmount)})`);
  }

  // approve
  const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));
  console.log("\nApproving wZION...");
  const tx1 = await wzion.approve(stakingAddr, seedAmount);
  await tx1.wait(2);
  await sleep(3000);
  console.log(`✅ Approved: ${tx1.hash}`);

  // fund
  console.log("Funding reward pool...");
  const tx2 = await staking.fundRewardPool(seedAmount);
  await tx2.wait(2);
  await sleep(3000);
  console.log(`✅ Funded: ${tx2.hash}`);

  const pool = await staking.rewardPoolBalance();
  console.log(`✅ Staking rewardPoolBalance: ${ethers.formatEther(pool)} wZION`);
}
main().catch(e => { console.error(e); process.exit(1); });
