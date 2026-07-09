# ZION DeFi Contracts — Hardhat Deploy (3.0.4)

Tento adresář obsahuje Hardhat deploy projekt pro ZION DeFi kontrakty na Base Mainnet.

Source soubory byly kanonizovány (zkopírovány z `archive/2.9.9/legacy-code/L2/contracts/`) — tento adresář je nyní single source of truth pro V3 DeFi deploy.

## Aktuální stav adresáře

```
hardhat/
├── .env.mainnet.example   # template — zkopíruj do .env a vyplň secrets
├── .gitignore
├── README.md              # tento soubor
├── hardhat.config.ts      # Hardhat config (síť `base` = Base Mainnet)
├── package.json           # npm závislosti (hardhat, ethers, dotenv, ...)
├── tsconfig.json          # TS config pro deploy skripty
├── sol/                   # Solidity kontrakty (kanonizované)
│   ├── wZION.sol              # ERC-20 wrapper (již deployed na Base)
│   ├── ZIONBridge.sol         # L1↔L2 bridge (již deployed)
│   ├── ZIONAtomicSwap.sol     # atomic swap (již deployed)
│   ├── ZIONStaking.sol        # single-asset staking (NOVÝ 3.0.4 deploy)
│   ├── ZIONFarm.sol           # multi-pool yield farming (NOVÝ 3.0.4 deploy)
│   ├── ZIONGovernance.sol     # on-chain governance (NOVÝ 3.0.4 deploy)
│   └── ZIONTreasury.sol       # multisig treasury (NOVÝ 3.0.4 deploy)
└── scripts/               # Hardhat deploy/fund/verify skripty
    ├── deploy-defi.ts                 # deploy Governance + Treasury + Staking (Base)
    ├── deploy-farm.ts                 # deploy ZIONFarm + init pooly (Base)
    ├── deploy-chain.ts                # deploy wZION + ZIONBridge on ANY EVM chain
    ├── fund-staking.ts                # fund staking reward pool
    ├── fund-farm.ts                   # fund farm reward pool
    └── verify-base-mainnet-basescan.ts # Basescan source verification
```

> **Multi-chain deploy:** `deploy-chain.ts` works for Arbitrum, BSC, Polygon, Optimism, Avalanche:
> ```bash
> npx hardhat run scripts/deploy-chain.ts --network arbitrum   # needs ETH on Arbitrum
> npx hardhat run scripts/deploy-chain.ts --network bsc        # needs BNB for gas
> npx hardhat run scripts/deploy-chain.ts --network polygon    # needs POL for gas
> npx hardhat run scripts/deploy-chain.ts --network optimism   # needs ETH on OP
> npx hardhat run scripts/deploy-chain.ts --network avalanche  # needs AVAX for gas
> ```

> **Pozn.:** `wZION.sol`, `ZIONBridge.sol`, `ZIONAtomicSwap.sol` jsou zahrnuty pro referenci / kompilaci závislostí — tyto kontrakty jsou **již deployed** na Base Mainnet (adresy v `.env.mainnet.example`). Nový 3.0.4 deploy cílí pouze na `ZIONStaking`, `ZIONFarm`, `ZIONGovernance`, `ZIONTreasury`.

## Kontrakty k deployi (3.0.4)

| Kontrakt | Popis | Prerequisita |
|----------|-------|-------------|
| `ZIONGovernance.sol` | On-chain governance (proposals, voting weight) | wZION |
| `ZIONTreasury.sol` | Multisig treasury (5-of-7 po provisioning) | wZION |
| `ZIONStaking.sol` | Single-asset staking (wZION → wZION rewards, 12% APR, 7d cooldown) | wZION |
| `ZIONFarm.sol` | Multi-pool yield farming (MasterChef v2, halving každých 90d) | wZION |

## Stav

- **Sepolia testnet:** deployed 2026-03-02 (viz `archive/2.9.9/legacy-code/L2/contracts/deployed-*.json` pro adresy)
- **Base Mainnet:** ❌ PENDING — čeká na 3.0.4 deploy (~0.005 ETH gas potřeba)

## Prerekvizity

- Node.js 18+
- Deployer wallet `0xdde17506...` s ~0.005 ETH na Base Mainnet
- Basescan API key

## Rychlý start

```bash
# 1. Nastav environment
cp .env.mainnet.example .env
# Vyplň DEPLOYER_PRIVATE_KEY, BASE_MAINNET_RPC, BASESCAN_API_KEY

# 2. Instaluj závislosti
npm install

# 3. Deploy (Base Mainnet)
npx hardhat run scripts/deploy-defi.ts --network base    # → deployed-defi.json
npx hardhat run scripts/deploy-farm.ts --network base    # → deployed-farm-base.json

# 4. Fund reward pools
npx hardhat run scripts/fund-staking.ts --network base
npx hardhat run scripts/fund-farm.ts --network base

# 5. Verify na Basescan
npx hardhat run scripts/verify-base-mainnet-basescan.ts --network base
```

## Po deployi — aktualizuj web konfiguraci

```typescript
// APP&WEB/website-v2.9/src/lib/defi-contracts.ts
// Base Mainnet (deployed 3.0.4)
ZIONStaking:    '0x<from deployed-defi.json>',
ZIONFarm:       '0x<from deployed-farm-base.json>',
ZIONGovernance: '0x<from deployed-defi.json>',
ZIONTreasury:   '0x<from deployed-defi.json>',

export const STAKING_DEPLOYED    = true;
export const FARM_DEPLOYED       = true;
export const GOVERNANCE_DEPLOYED = true;
```

## Parametry kontraktů

### ZIONStaking
- **Min stake:** 100 wZION
- **Cooldown:** 7 dní (před unstake)
- **APR:** 12% initial (konfigurovatelné, max 50%)
- **Voting weight:** staked balance = governance vote power

### ZIONFarm
- **Pool 0:** wZION single-asset (100 alloc pts)
- **Pool 1:** wZION/USDT LP positions (50 alloc pts, přidá se po deploy)
- **Reward rate:** 1 wZION/s initial (~86,400 wZION/den)
- **Halving:** každých 90 dní

## Provenience

Source soubory kanonizovány z `archive/2.9.9/legacy-code/L2/contracts/` (P6 kanonizace).
Původní Sepolia deploy artefakty zůstávají v archive:
- `archive/2.9.9/legacy-code/L2/contracts/deployed-defi.json` — Sepolia testnet deploy (2026-03-02)
- `archive/2.9.9/legacy-code/L2/contracts/deployed-farm-base-sepolia.json` — Sepolia farm deploy (2026-03-02)
