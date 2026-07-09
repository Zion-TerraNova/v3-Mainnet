/**
 * deploy-sefirot-vow.ts — Deploy SefirotVowToken (soulbound)
 *
 * Deploys the Sefirot Vow soulbound ERC-721 token. The authorized minter
 * is set to the ZIONGovernance contract (or deployer on testnet).
 *
 * Usage:
 *   npx hardhat run scripts/deploy-sefirot-vow.ts --network base
 *   npx hardhat run scripts/deploy-sefirot-vow.ts --network base-sepolia
 *
 * Required env vars:
 *   DEPLOYER_PRIVATE_KEY  — signer (set in .env)
 *
 * Optional:
 *   GOVERNANCE_ADDRESS    — ZIONGovernance contract (defaults to deployer on testnet)
 */

import { ethers, network } from "hardhat";
import * as fs from "fs";
import * as path from "path";

async function main() {
  const networkName = network.name;
  const isLocal = networkName === "hardhat" || networkName === "localhost";
  const { chainId } = await ethers.provider.getNetwork();

  console.log("\n" + "=".repeat(70));
  console.log("  ZION Sefirot Vow Token Deploy (soulbound ERC-721)");
  console.log("=".repeat(70));
  console.log(`Network:  ${networkName} (chain ${chainId})`);

  const [deployer] = await ethers.getSigners();
  const balance = await ethers.provider.getBalance(deployer.address);
  console.log(`Deployer: ${deployer.address}`);
  console.log(`Balance:  ${ethers.formatEther(balance)} ETH`);

  if (balance === 0n && !isLocal) {
    console.error("\nFATAL: Deployer has no ETH for gas.");
    process.exit(1);
  }

  // ─── Authorized minter ──────────────────────────────────────────────
  // On mainnet, this should be the ZIONGovernance contract address.
  // On testnet/local, default to deployer for testing.
  const governanceAddr = process.env.GOVERNANCE_ADDRESS || deployer.address;
  console.log(`\nAuthorized minter: ${governanceAddr}`);
  console.log(`  (set GOVERNANCE_ADDRESS env to ZIONGovernance contract for mainnet)`);

  // ─── Deploy ─────────────────────────────────────────────────────────
  console.log("\nDeploying SefirotVowToken...");
  const Factory = await ethers.getContractFactory("SefirotVowToken");
  const token = await Factory.deploy(governanceAddr);
  await token.waitForDeployment();
  const tokenAddr = await token.getAddress();

  console.log(`\nSefirotVowToken deployed: ${tokenAddr}`);
  console.log(`  authorizedMinter: ${governanceAddr}`);
  console.log(`  name: ZION Sefirot Vow`);
  console.log(`  symbol: SEFIROT-VOW`);
  console.log(`  soulbound: non-transferable (only mint/burn)`);

  // ─── Save deployment record ─────────────────────────────────────────
  const record = {
    contract: "SefirotVowToken",
    address: tokenAddr,
    network: networkName,
    chainId: Number(chainId),
    authorizedMinter: governanceAddr,
    deployedAt: new Date().toISOString(),
    txHash: token.deploymentTransaction()?.hash,
    deployer: deployer.address,
  };

  const outDir = path.join(__dirname, "..", "deployments");
  if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });
  const outFile = path.join(outDir, `sefirot-vow-${networkName}.json`);
  fs.writeFileSync(outFile, JSON.stringify(record, null, 2));
  console.log(`\nDeployment record saved: ${outFile}`);

  // ─── Verify on Basescan (mainnet only) ──────────────────────────────
  if (networkName === "base" && process.env.BASESCAN_API_KEY) {
    console.log("\nVerifying on Basescan...");
    try {
      await (hre as any).run("verify:verify", {
        address: tokenAddr,
        constructorArguments: [governanceAddr],
      });
      console.log("Basescan verification: OK");
    } catch (e: any) {
      console.log(`Basescan verification failed: ${e.message}`);
    }
  }

  console.log("\n" + "=".repeat(70));
  console.log("  Sefirot Vow Token deploy complete.");
  console.log("  See: V3/L5/docs/GOVERNANCE/sefirot-vow.md");
  console.log("=".repeat(70) + "\n");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
