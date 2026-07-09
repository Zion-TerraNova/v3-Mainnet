/**
 * init-pancakeswap-pool.ts — Initialize + add liquidity to existing PancakeSwap V3 pool
 *
 * Pool already created at: 0x46cc98dec9d2a60f2850225c942d6017b82b6f47
 * This script initializes it and adds liquidity with available USDT.
 *
 * Usage:
 *   npx hardhat run scripts/init-pancakeswap-pool.ts --network base
 */

import { ethers, network } from "hardhat";

// Pool address (from PoolCreated event in TX 0x17756943...)
const POOL_ADDRESS = "0x46cc98dec9d2a60f2850225c942d6017b82b6f47";
const PANCAKE_V3_NFT_POSITION_MANAGER = "0x46A15B0b27311cedF172AB29E4f4766fbE7F4364";

const WZION = "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6";
const USDT  = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2";

// Seed price: $0.0002 / wZION
// token0 = wZION (18 dec), token1 = USDT (6 dec)
// price = USDT/wZION = 0.0002
// sqrtPriceX96 = sqrt(0.0002 * 10^(18-6)) * 2^96 = sqrt(2e8) * 2^96
const SEED_SQRT_PRICE_X96 = "1120455419495722778624";

// Full range for 0.25% fee (tickSpacing = 50)
const TICK_LOWER = -887200;
const TICK_UPPER = 887200;

const POOL_ABI = [
  "function initialize(uint160 sqrtPriceX96) external",
  "function slot0() external view returns (uint160 sqrtPriceX96, int24 tick, uint16 observationIndex, uint16 observationCardinality, uint16 observationCardinalityNext, uint8 feeProtocol, bool unlocked)",
  "function liquidity() external view returns (uint128)",
  "function token0() external view returns (address)",
  "function token1() external view returns (address)",
  "function fee() external view returns (uint24)",
];

const NFT_ABI = [
  "function mint((address token0, address token1, uint24 fee, int24 tickLower, int24 tickUpper, uint256 amount0Desired, uint256 amount1Desired, uint256 amount0Min, uint256 amount1Min, address recipient, uint256 deadline)) external payable returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1)",
];

const ERC20_ABI = [
  "function balanceOf(address) view returns (uint256)",
  "function approve(address spender, uint256 amount) returns (bool)",
  "function allowance(address owner, address spender) view returns (uint256)",
  "function decimals() view returns (uint8)",
];

async function main() {
  const [deployer] = await ethers.getSigners();
  const balance = await ethers.provider.getBalance(deployer.address);

  console.log("\n" + "═".repeat(70));
  console.log("  Init PancakeSwap V3 Pool + Add Liquidity");
  console.log("═".repeat(70));
  console.log(`Deployer: ${deployer.address}`);
  console.log(`Balance:  ${ethers.formatEther(balance)} ETH`);
  console.log(`Pool:     ${POOL_ADDRESS}`);

  const pool = new ethers.Contract(POOL_ADDRESS, POOL_ABI, deployer);
  const wzion = new ethers.Contract(WZION, ERC20_ABI, deployer);
  const usdt = new ethers.Contract(USDT, ERC20_ABI, deployer);

  // Token info
  const token0 = await pool.token0();
  const token1 = await pool.token1();
  const fee = await pool.fee();
  const isWzionToken0 = token0.toLowerCase() === WZION.toLowerCase();

  console.log(`token0:  ${token0} (${isWzionToken0 ? "wZION" : "USDT"})`);
  console.log(`token1:  ${token1} (${isWzionToken0 ? "USDT" : "wZION"})`);
  console.log(`fee:     ${fee}`);

  // Check balances
  const wzBal = await wzion.balanceOf(deployer.address);
  const usdtBal = await usdt.balanceOf(deployer.address);
  console.log(`\nwZION:   ${ethers.formatUnits(wzBal, 18)}`);
  console.log(`USDT:    ${ethers.formatUnits(usdtBal, 6)}`);

  // ── Step 1: Initialize ──────────────────────────────────────────────────
  let slot0;
  try {
    slot0 = await pool.slot0();
  } catch {
    slot0 = null;
  }

  if (slot0 && slot0.sqrtPriceX96 > 0n) {
    console.log(`\n[1/3] Pool already initialized (sqrtPriceX96: ${slot0.sqrtPriceX96})`);
  } else {
    console.log("\n[1/3] Initializing pool...");
    console.log(`  sqrtPriceX96: ${SEED_SQRT_PRICE_X96}`);
    const tx = await pool.initialize(BigInt(SEED_SQRT_PRICE_X96));
    console.log(`  TX: ${tx.hash}`);
    await tx.wait();
    console.log(`  ✓ Pool initialized at $0.0002/wZION`);
  }

  if (usdtBal < 1000n) {
    // Less than 0.001 USDT — skip liquidity
    console.log("\n[2/3] Insufficient USDT for liquidity, skipping");
    console.log("[3/3] Skipped");
    console.log(`\nPool address: ${POOL_ADDRESS}`);
    console.log(`Basescan: https://basescan.org/address/${POOL_ADDRESS}`);
    return;
  }

  // ── Step 2: Approve ─────────────────────────────────────────────────────
  console.log("\n[2/3] Approving tokens...");

  // Use all available USDT, calculate matching wZION
  const usdtAmount = usdtBal;
  // At $0.0002/wZION: wZION = USDT / 0.0002 = USDT * 5000
  // USDT has 6 decimals, wZION has 18 decimals
  // wzionAmount (18 dec) = usdtAmount (6 dec) * 5000 * 10^12
  const wzionAmount = (usdtAmount * 5000n * (10n ** 12n)) / 1_000_000n;

  console.log(`  wZION to add: ${ethers.formatUnits(wzionAmount, 18)}`);
  console.log(`  USDT to add:  ${ethers.formatUnits(usdtAmount, 6)}`);

  // Approve wZION
  const wzAllow = await wzion.allowance(deployer.address, PANCAKE_V3_NFT_POSITION_MANAGER);
  if (wzAllow < wzionAmount) {
    const tx = await wzion.approve(PANCAKE_V3_NFT_POSITION_MANAGER, ethers.MaxUint256);
    await tx.wait();
    console.log(`  ✓ wZION approved`);
    // Wait for RPC to clear pending tx state
    await new Promise(r => setTimeout(r, 3000));
  }

  // Approve USDT
  const usdtAllow = await usdt.allowance(deployer.address, PANCAKE_V3_NFT_POSITION_MANAGER);
  if (usdtAllow < usdtAmount) {
    const tx = await usdt.approve(PANCAKE_V3_NFT_POSITION_MANAGER, ethers.MaxUint256);
    await tx.wait();
    console.log(`  ✓ USDT approved`);
    await new Promise(r => setTimeout(r, 3000));
  }

  // ── Step 3: Add liquidity ───────────────────────────────────────────────
  console.log("\n[3/3] Adding liquidity...");

  const nftManager = new ethers.Contract(PANCAKE_V3_NFT_POSITION_MANAGER, NFT_ABI, deployer);

  const amount0Desired = isWzionToken0 ? wzionAmount : usdtAmount;
  const amount1Desired = isWzionToken0 ? usdtAmount : wzionAmount;

  const deadline = Math.floor(Date.now() / 1000) + 3600;

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

  const tx = await nftManager.mint(mintParams);
  console.log(`  TX: ${tx.hash}`);
  const receipt = await tx.wait();

  // Find IncreaseLiquidity event
  const event = receipt?.logs.find((log) => {
    try {
      const parsed = nftManager.interface.parseLog(log);
      return parsed?.name === "IncreaseLiquidity";
    } catch { return false; }
  });

  let tokenId: string | undefined;
  if (event) {
    const parsed = nftManager.interface.parseLog(event);
    tokenId = parsed?.args?.tokenId?.toString();
  }

  console.log(`  ✓ Liquidity added! NFT Position ID: ${tokenId ?? "unknown"}`);

  console.log("\n" + "═".repeat(70));
  console.log("  ✅ PancakeSwap V3 Pool Ready!");
  console.log("═".repeat(70));
  console.log(`  Pool:       ${POOL_ADDRESS}`);
  console.log(`  Fee:        0.25%`);
  console.log(`  NFT ID:     ${tokenId ?? "unknown"}`);
  console.log(`  Basescan:   https://basescan.org/address/${POOL_ADDRESS}`);
  console.log(`  Swap:       https://pancakeswap.finance/swap?outputCurrency=${WZION}&chain=base`);
  console.log(`  Add Liq:    https://pancakeswap.finance/addV3/${POOL_ADDRESS}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
