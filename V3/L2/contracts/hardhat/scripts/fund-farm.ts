/**
 * Seed ZIONFarm reward pool with wZION
 *
 * Usage:
 *   npx hardhat run scripts/fund-farm.ts --network base
 *
 * Env vars:
 *   FARM_ADDRESS     — deployed ZIONFarm address (overrides deployed-farm-base.json)
 *   WZION_ADDRESS    — wZION token address (default: Base Mainnet)
 *   FARM_SEED_AMOUNT — wZION to seed (default: 500000 = 500K wZION)
 */
import { ethers } from "hardhat";
import * as fs from "fs";
import * as path from "path";

const WZION_DEFAULT = "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6";
const DEFAULT_SEED = "500000"; // 500K wZION

async function main() {
  const [deployer] = await ethers.getSigners();
  console.log(`\n🌾 Seed ZIONFarm — deployer: ${deployer.address}`);

  // Resolve farm address from env or deployed-farm-base.json
  let farmAddr = process.env.FARM_ADDRESS || "";
  if (!farmAddr) {
    const deployedPath = path.join(__dirname, "..", "deployed-farm-base.json");
    if (fs.existsSync(deployedPath)) {
      const deployed = JSON.parse(fs.readFileSync(deployedPath, "utf8"));
      farmAddr = deployed.farm || deployed.ZIONFarm || "";
    }
  }
  if (!farmAddr || !ethers.isAddress(farmAddr)) {
    throw new Error(
      "FARM_ADDRESS not set. Either:\n" +
      "  1. Set FARM_ADDRESS env var, or\n" +
      "  2. Run deploy-farm.ts first to generate deployed-farm-base.json"
    );
  }

  const wzionAddr = process.env.WZION_ADDRESS || WZION_DEFAULT;
  const seedAmount = ethers.parseEther(process.env.FARM_SEED_AMOUNT || DEFAULT_SEED);

  console.log(`Farm:     ${farmAddr}`);
  console.log(`wZION:    ${wzionAddr}`);
  console.log(`Seed:     ${ethers.formatEther(seedAmount)} wZION`);

  const wzion = await ethers.getContractAt("WZION", wzionAddr);
  const farm  = await ethers.getContractAt("ZIONFarm", farmAddr);

  const bal = await wzion.balanceOf(deployer.address);
  console.log(`Deployer wZION balance: ${ethers.formatEther(bal)}`);
  if (bal < seedAmount) {
    throw new Error(`Insufficient wZION (have ${ethers.formatEther(bal)}, need ${ethers.formatEther(seedAmount)})`);
  }

  // Use high gas to clear any stuck pending txs
  const gasOpts = {
    maxFeePerGas: ethers.parseUnits("10", "gwei"),
    maxPriorityFeePerGas: ethers.parseUnits("2", "gwei"),
  };

  // approve
  const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));
  console.log("\nApproving...");
  const tx1 = await wzion.approve(farmAddr, seedAmount, gasOpts);
  await tx1.wait(2);
  await sleep(3000);
  console.log(`✅ Approved: ${tx1.hash}`);

  // fundRewards
  console.log("Funding rewards...");
  const tx2 = await farm.fundRewards(seedAmount, gasOpts);
  await tx2.wait(2);
  await sleep(3000);
  console.log(`✅ Funded: ${tx2.hash}`);

  const pool = await farm.rewardPoolBalance();
  console.log(`✅ Farm rewardPoolBalance: ${ethers.formatEther(pool)} wZION`);
}
main().catch(e => { console.error(e); process.exit(1); });
