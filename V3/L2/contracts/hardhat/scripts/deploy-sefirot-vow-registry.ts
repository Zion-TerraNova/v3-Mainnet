/**
 * deploy-sefirot-vow-registry.ts — Deploy SefirotVowRegistry
 *
 * Deploys the SefirotVowRegistry and links it to an already-deployed
 * SefirotVowToken as the authorized minter.
 *
 * Usage:
 *   npx hardhat run scripts/deploy-sefirot-vow-registry.ts --network base
 *
 * Required:
 *   DEPLOYER_PRIVATE_KEY
 *   SEFIROT_VOW_TOKEN_ADDRESS  — from deploy-sefirot-vow.ts
 *
 * Optional:
 *   INITIAL_VALIDATORS  — comma-separated addresses to authorize as bootstrap validators
 */

import { ethers, network } from "hardhat";
import * as fs from "fs";
import * as path from "path";

async function main() {
  const networkName = network.name;
  const isLocal = networkName === "hardhat" || networkName === "localhost";
  const { chainId } = await ethers.provider.getNetwork();

  console.log("\n" + "=".repeat(70));
  console.log("  ZION Sefirot Vow Registry Deploy");
  console.log("=".repeat(70));
  console.log(`Network:  ${networkName} (chain ${chainId})`);

  const [deployer] = await ethers.getSigners();
  console.log(`Deployer: ${deployer.address}`);

  // ─── Resolve token address ──────────────────────────────────────────
  let tokenAddr = process.env.SEFIROT_VOW_TOKEN_ADDRESS;
  if (!tokenAddr) {
    // Try to read from deployment record
    const recordPath = path.join(__dirname, "..", "deployments", `sefirot-vow-${networkName}.json`);
    if (fs.existsSync(recordPath)) {
      const record = JSON.parse(fs.readFileSync(recordPath, "utf8"));
      tokenAddr = record.address;
    }
  }
  if (!tokenAddr) {
    console.error("\nFATAL: SEFIROT_VOW_TOKEN_ADDRESS not set and no deployment record found.");
    console.error("Run deploy-sefirot-vow.ts first.");
    process.exit(1);
  }
  console.log(`SefirotVowToken: ${tokenAddr}`);

  // ─── Deploy registry ────────────────────────────────────────────────
  console.log("\nDeploying SefirotVowRegistry...");
  const Factory = await ethers.getContractFactory("SefirotVowRegistry");
  const registry = await Factory.deploy(tokenAddr);
  await registry.waitForDeployment();
  const registryAddr = await registry.getAddress();
  console.log(`SefirotVowRegistry deployed: ${registryAddr}`);

  // ─── Link: set registry as authorized minter on token ───────────────
  console.log("\nLinking registry as authorized minter on token...");
  const token = await ethers.getContractAt("SefirotVowToken", tokenAddr);
  const tx = await token.setAuthorizedMinter(registryAddr);
  await tx.wait();
  console.log(`token.setAuthorizedMinter(${registryAddr}) — tx: ${tx.hash}`);

  // ─── Authorize bootstrap validators ─────────────────────────────────
  const initialValidators = process.env.INITIAL_VALIDATORS?.split(",").map(s => s.trim()).filter(Boolean) ?? [];
  if (initialValidators.length > 0) {
    console.log(`\nAuthorizing ${initialValidators.length} bootstrap validators...`);
    for (const v of initialValidators) {
      const tx2 = await registry.setAuthorizedValidator(v, true);
      await tx2.wait();
      console.log(`  authorized: ${v}`);
    }
  } else {
    console.log("\nNo INITIAL_VALIDATORS set — authorize validators manually after deploy.");
  }

  // ─── Save deployment record ─────────────────────────────────────────
  const record = {
    contract: "SefirotVowRegistry",
    address: registryAddr,
    network: networkName,
    chainId: Number(chainId),
    vowToken: tokenAddr,
    deployedAt: new Date().toISOString(),
    txHash: registry.deploymentTransaction()?.hash,
    deployer: deployer.address,
    initialValidators,
  };

  const outDir = path.join(__dirname, "..", "deployments");
  if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });
  const outFile = path.join(outDir, `sefirot-vow-registry-${networkName}.json`);
  fs.writeFileSync(outFile, JSON.stringify(record, null, 2));
  console.log(`\nDeployment record saved: ${outFile}`);

  console.log("\n" + "=".repeat(70));
  console.log("  Sefirot Vow Registry deploy complete.");
  console.log("=".repeat(70) + "\n");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
