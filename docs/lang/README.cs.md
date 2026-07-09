# ZION

<div align="center">

<img src="../../docs/stargate/nebula.jpg" width="260" height="260" alt="ZION Stargate" style="border-radius: 50%; object-fit: cover; box-shadow: 0 0 50px rgba(0,180,255,0.25);" />

<br/>

## Terra Nova — 100 let evoluZionu

**Multichain Dharma ekosystém zabezpečený proof-of-work konsenzem.**

[www.zionterranova.com](https://www.zionterranova.com)

<br/>

</div>

ZION je vícevrstvý blockchain: L1 PoW jádro, L2 DeFi a cross-chain bridge, L3 WARP a Hiran AI a L4 Oasis — duchovní MMORPG s těžbou vědomí.

Tento repozitář obsahuje kód hlavní sítě v3. Aktuálně je v **Mainnet Beta**: živá, produkuje bloky a těžba je na vlastní nebezpečí.

---

## Vstupte do Oasisu

| Portál | Cesta |
|---|---|
| **Těžit** | Spusť uzel nebo miner na ZION L1. Začni v [`V3/cli/README.md`](../../V3/cli/README.md). |
| **Hrát** | Vstup do světa L4 Oasis — avatary, úkoly, gildy a Golden Egg. Viz [`V3/L4/oasis/README.md`](../../V3/L4/oasis/README.md). |
| **Stavět** | Prozkoumej kód, kontrakty, RPC a bridge dokumentaci v [`V3/docs/`](../../V3/docs/) a [`docs/`](../../docs/). |

---

## Status sítě

> **Mainnet Beta — živá na vlastní nebezpečí**

| Parametr | Hodnota |
|---|---|
| Status | Mainnet Beta |
| Protokol | 3.0.4 |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Oficiální launch | 2026-12-31 |

Všechny zveřejněné bezpečnostní problémy byly remediovány. Viz [Security](../../SECURITY.md) a [disclosure report](../../docs/security/SECURITY_DISCLOSURE_2026-07.md).

---

## Rychlý start

```bash
# Sestav L1 uzel
cargo build --release

# Spusť uzel
cargo run --release -p zion-core --bin zion-node

# Přečti celý příběh
open ../../README_FULL.md
```

---

## Jazyky

[English](../../README.md) · **Čeština** · [Español](./README.es.md) · [Français](./README.fr.md) · [Português](./README.pt.md)

---

## Plná dokumentace

Kompletní přehled architektury, funkcí, historie a roadmapy najdeš v **[README_FULL.md](../../README_FULL.md)**.

---

## Licence

Tento projekt je licencován pod [MIT Licencí](../../LICENSE).

<div align="center">

Postaveno s péčí, zabezpečeno konsenzem.

</div>
