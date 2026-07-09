/**
 * deploy-pancakeswap-pool.ts — Create wZION/USDT pool on PancakeSwap V3 (Base)
 *
 * PancakeSwap V3 is the 2nd largest DEX on Base after Aerodrome.
 * This script creates a wZION/USDT pool with 0.25% fee (PancakeSwap's standard),
 * initializes it at the seed price ($0.0002/wZION), and adds liquidity.
 *
 * Usage:
 *   npx hardhat run scripts/deploy-pancakeswap-pool.ts --network base
 *
 * Required env vars:
 *   DEPLOYER_PRIVATE_KEY  — signer (set in .env)
 *
 * Optional:
 *   WZION_AMOUNT  — wZION to add as liquidity (default: 50000)
 *   USDT_AMOUNT   — USDT to add as liquidity (default: 10)
 *   POOL_FEE      — fee tier (default: 2500 = 0.25%)
 *
 * PancakeSwap V3 contracts on Base:
 *   Factory:                  0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865
 *   PoolDeployer:             0x41ff9AA7e16B8B1a8a8dc4f0eFacd93D02d071c9
 *   NonfungiblePositionManager: 0x46A15B0b27311cedF172AB29E4f4766fbE7F4364
 *   SwapRouter:               0x1b81D678ffb9C0263b24A97847620C99d213eB14
 *   QuoterV2:                 0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997
 *   Smart Router:             0x678Aa4bF4E210cf2166753e054d5b7c31cc7fa86
 */

import { ethers, network } from "hardhat";
import * as fs from "fs";
import * as path from "path";

// ─── PancakeSwap V3 contracts on Base ─────────────────────────────────────────
const PANCAKE_V3_FACTORY = "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865";
const PANCAKE_V3_NFT_POSITION_MANAGER = "0x46A15B0b27311cedF172AB29E4f4766fbE7F4364";

// ─── Token addresses on Base ──────────────────────────────────────────────────
const WZION = "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6";
const USDT  = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2";

// ─── Seed price: $0.0002 / wZION ──────────────────────────────────────────────
// wZION (token0, 18 decimals) < USDT (token1, 6 decimals) by address
// Price = USDT per wZION = 0.0002
// Raw price = 0.0002 * 10^(18-6) = 0.0002 * 10^12 = 2e8
// sqrtPriceX96 = sqrt(2e8) * 2^96 = 1120455419495722778624
// tick = floor(log(2e8) / log(1.0001)) = -361501
const SEED_SQRT_PRICE_X96 = "1120455419495722778624";
const SEED_TICK = -361501;

// Full range tick bounds (for 0.25% fee, tickSpacing = 50)
const TICK_LOWER = -887200;
const TICK_UPPER = 887200;

// ─── ABIs (minimal) ───────────────────────────────────────────────────────────
const FACTORY_ABI = [
  "function createPool(address tokenA, address tokenB, uint24 fee) external returns (address pool)",
  "function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool)",
];

const POOL_ABI = [
  "function initialize(uint160 sqrtPriceX96) external",
  "function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)",
  "function liquidity() external view returns (uint128)",
  "function token0() external view returns (address)",
  "function token1() external view returns (address)",
  "function fee() external view returns (uint24)",
];

const NFT_POSITION_MANAGER_ABI = [
  "function mint((address token0, address token1, uint24 fee, int24 tickLower, int24 tickUpper, uint256 amount0Desired, uint256 amount1Desired, uint256 amount0Min, uint256 amount1Min, address recipient, uint256 deadline)) external payable returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1)",
  "function positions(uint256 tokenId) external view returns (uint96 nonce, address operator, address token0, address token1, uint24 fee, int24 tickLower, int24 tickUpper, uint128 liquidity, uint256 feeGrowthInside0LastX128, uint256 feeGrowthInside1LastX128, uint128 tokensOwed0, uint128 tokensOwed1)",
];

const ERC20_ABI = [
  "function balanceOf(address) view returns (uint256)",
  "function approve(address spender, uint256 amount) returns (bool)",
  "function allowance(address owner, address spender) view returns (uint256)",
  "function decimals() view returns (uint8)",
];

// ─────────────────────────────────────────────────────────────────────────────

async function main() {
  const networkName = network.name;
  const { chainId } = await ethers.provider.getNetwork();

  console.log("\n" + "═".repeat(70));
  console.log("  PancakeSwap V3 Pool Deploy — wZION/USDT on Base");
  console.log("═".repeat(70));
  console.log(`Network:  ${networkName} (chain ${chainId})`);

  const [deployer] = await ethers.getSigners();
  const balance = await ethers.provider.getBalance(deployer.address);
  console.log(`Deployer: ${deployer.address}`);
  console.log(`Balance:  ${ethers.formatEther(balance)} ETH`);

  if (balance < ethers.parseEther("0.001")) {
    console.error("❌ Insufficient ETH balance (need ≥ 0.001 ETH for gas)");
    process.exit(1);
  }

  // ── Config ──────────────────────────────────────────────────────────────
  const poolFee = parseInt(process.env.POOL_FEE || "2500"); // 0.25%
  const wzionAmount = ethers.parseUnits(process.env.WZION_AMOUNT || "50000", 18);
  const usdtAmount = ethers.parseUnits(process.env.USDT_AMOUNT || "10", 6);

  console.log(`\nPool config:`);
  console.log(`  Fee:      ${poolFee / 10000}% (${poolFee})`);
  console.log(`  wZION:    ${ethers.formatUnits(wzionAmount, 18)}`);
  console.log(`  USDT:     ${ethers.formatUnits(usdtAmount, 6)}`);

  // ── Check token balances ────────────────────────────────────────────────
  const wzion = new ethers.Contract(WZION, ERC20_ABI, deployer);
  const usdt  = new ethers.Contract(USDT, ERC20_ABI, deployer);

  const wzBal = await wzion.balanceOf(deployer.address);
  const usdtBal = await usdt.balanceOf(deployer.address);

  console.log(`\nDeployer balances:`);
  console.log(`  wZION: ${ethers.formatUnits(wzBal, 18)}`);
  console.log(`  USDT:  ${ethers.formatUnits(usdtBal, 6)}`);

  if (wzBal < wzionAmount) {
    console.error(`❌ Insufficient wZION (need ${ethers.formatUnits(wzionAmount, 18)}, have ${ethers.formatUnits(wzBal, 18)})`);
    process.exit(1);
  }
  if (usdtBal < usdtAmount) {
    console.warn(`⚠️  Insufficient USDT (need ${ethers.formatUnits(usdtAmount, 6)}, have ${ethers.formatUnits(usdtBal, 6)})`);
    console.warn(`   Pool will be created and initialized, but liquidity will be skipped.`);
    console.warn(`   Send USDT to ${deployer.address} and re-run to add liquidity.`);
  }

  // ── Step 1: Create pool ─────────────────────────────────────────────────
  const factory = new ethers.Contract(PANCAKE_V3_FACTORY, FACTORY_ABI, deployer);

  // Check if pool already exists
  let poolAddress = await factory.getPool(WZION, USDT, poolFee);

  if (poolAddress === ethers.ZeroAddress) {
    console.log("\n[1/4] Creating pool...");
    const tx = await factory.createPool(WZION, USDT, poolFee);
    console.log(`  TX: ${tx.hash}`);
    const receipt = await tx.wait();
    console.log(`  ✓ Pool created (block ${receipt?.blockNumber})`);

    poolAddress = await factory.getPool(WZION, USDT, poolFee);
    console.log(`  Pool address: ${poolAddress}`);
  } else {
    console.log(`\n[1/4] Pool already exists: ${poolAddress}`);
  }

  // ── Step 2: Initialize pool ─────────────────────────────────────────────
  const pool = new ethers.Contract(poolAddress, POOL_ABI, deployer);

  const token0 = await pool.token0();
  const token1 = await pool.token1();
  const fee = await pool.fee();
  console.log(`\n  token0: ${token0}`);
  console.log(`  token1: ${token1}`);
  console.log(`  fee:    ${fee}`);

  // Check if pool is already initialized
  let slot0;
  try {
    slot0 = await pool.slot0();
  } catch {
    slot0 = null;
  }

  if (slot0 && slot0.sqrtPriceX96 > 0n) {
    console.log(`\n[2/4] Pool already initialized (sqrtPriceX96: ${slot0.sqrtPriceX96})`);
  } else {
    console.log("\n[2/4] Initializing pool...");
    const sqrtPriceX96 = BigInt(SEED_SQRT_PRICE_X96);
    console.log(`  sqrtPriceX96: ${sqrtPriceX96}`);
    console.log(`  Price: $0.0002/wZION`);
    const tx = await pool.initialize(sqrtPriceX96);
    console.log(`  TX: ${tx.hash}`);
    await tx.wait();
    console.log(`  ✓ Pool initialized`);
  }

  // ── Step 3: Approve tokens ──────────────────────────────────────────────
  if (usdtBal < usdtAmount) {
    console.log("\n[3/4] Skipping liquidity (insufficient USDT)");
    console.log("\n[4/4] Skipping liquidity addition");
    console.log("\n═".repeat(70));
    console.log(`  Pool deployed: ${poolAddress}`);
    console.log(`  PancakeSwap:   https://pancakeswap.finance/addV3/${poolAddress}`);
    console.log(`  Basescan:      https://basescan.org/address/${poolAddress}`);
    console.log("═".repeat(70));

    // Save deployment info
    saveDeployment(poolAddress, poolFee, networkName, chainId);
    return;
  }

  console.log("\n[3/4] Approving tokens to NFT Position Manager...");

  // Approve wZION
  const wzAllowance = await wzion.allowance(deployer.address, PANCAKE_V3_NFT_POSITION_MANAGER);
  if (wzAllowance < wzionAmount) {
    const tx = await wzion.approve(PANCAKE_V3_NFT_POSITION_MANAGER, ethers.MaxUint256);
    await tx.wait();
    console.log(`  ✓ wZION approved`);
  } else {
    console.log(`  ✓ wZION already approved`);
  }

  // Approve USDT
  const usdtAllowance = await usdt.allowance(deployer.address, PANCAKE_V3_NFT_POSITION_MANAGER);
  if (usdtAllowance < usdtAmount) {
    const tx = await usdt.approve(PANCAKE_V3_NFT_POSITION_MANAGER, ethers.MaxUint256);
    await tx.wait();
    console.log(`  ✓ USDT approved`);
  } else {
    console.log(`  ✓ USDT already approved`);
  }

  // ── Step 4: Add liquidity ───────────────────────────────────────────────
  console.log("\n[4/4] Adding liquidity...");

  const nftManager = new ethers.Contract(PANCAKE_V3_NFT_POSITION_MANAGER, NFT_POSITION_MANAGER_ABI, deployer);

  // Determine token order (token0 must be lower address)
  const isWzionToken0 = token0.toLowerCase() === WZION.toLowerCase();
  const amount0Desired = isWzionToken0 ? wzionAmount : usdtAmount;
  const amount1Desired = isWzionToken0 ? usdtAmount : wzionAmount;

  const deadline = Math.floor(Date.now() / 1000) + 3600; // 1 hour

  const mintParams = {
    token0: token0,
    token1: token1,
    fee: fee,
    tickLower: TICK_LOWER,
    tickUpper: TICK_UPPER,
    amount0Desired: amount0Desired,
    amount1Desired: amount1Desired,
    amount0Min: 0,
    amount1Min: 0,
    recipient: deployer.address,
    deadline: deadline,
  };

  console.log(`  amount0Desired: ${ethers.formatUnits(amount0Desired, isWzionToken0 ? 18 : 6)}`);
  console.log(`  amount1Desired: ${ethers.formatUnits(amount1Desired, isWzionToken0 ? 6 : 18)}`);

  const tx = await nftManager.mint(mintParams);
  console.log(`  TX: ${tx.hash}`);
  const receipt = await tx.wait();

  // Parse Mint event to get tokenId
  const mintEvent = receipt?.logs.find((log) => {
    try {
      const parsed = nftManager.interface.parseLog(log);
      return parsed?.name === "IncreaseLiquidity";
    } catch { return false; }
  });

  let tokenId: string | undefined;
  if (mintEvent) {
    const parsed = nftManager.interface.parseLog(mintEvent);
    tokenId = parsed?.args?.tokenId?.toString();
  }

  console.log(`  ✓ Liquidity added (NFT ID: ${tokenId ?? "unknown"})`);

  // ── Summary ─────────────────────────────────────────────────────────────
  console.log("\n" + "═".repeat(70));
  console.log("  ✅ PancakeSwap V3 Pool Deploy Complete!");
  console.log("═".repeat(70));
  console.log(`  Pool address:   ${poolAddress}`);
  console.log(`  Fee:            ${poolFee / 10000}%`);
  console.log(`  NFT Position:   #${tokenId ?? "unknown"}`);
  console.log(`  PancakeSwap:    https://pancakeswap.finance/addV3/${poolAddress}`);
  console.log(`  Basescan:       https://basescan.org/address/${poolAddress}`);
  console.log(`  Swap URL:       https://pancakeswap.finance/swap?outputCurrency=${WZION}&chain=base`);

  saveDeployment(poolAddress, poolFee, networkName, chainId, tokenId);
}

function saveDeployment(poolAddress: string, fee: number, network: string, chainId: bigint, tokenId?: string) {
  const deployedPath = path.join(__dirname, "..", "deployed-pancakeswap.json");
  const existing = fs.existsSync(deployedPath)
    ? JSON.parse(fs.readFileSync(deployedPath, "utf8"))
    : {};

  existing[`${network}-${chainId}`] = {
    pool: poolAddress,
    factory: PANCAKE_V3_FACTORY,
    nftPositionManager: PANCAKE_V3_NFT_POSITION_MANAGER,
    token0: WZION,
    token1: USDT,
    fee,
    feeLabel: `${fee / 10000}%`,
    nftTokenId: tokenId || null,
    deployedAt: new Date().toISOString(),
  };

  fs.writeFileSync(deployedPath, JSON.stringify(existing, null, 2));
  console.log(`\n  Saved to: ${deployedPath}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
