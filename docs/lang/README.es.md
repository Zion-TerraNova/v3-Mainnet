<div align="center">

<!-- Hero banner — Stargate OG image -->
<img src="../../docs/stargate/stargate-og.png" width="320" alt="ZION Stargate" />

<!-- Title -->
<h1>ZION</h1>

<h3>Terra Nova — 100 años de evoluZion</h3>

<p><em>Un ecosistema Dharma multichain asegurado por consenso proof-of-work.</em></p>

<!-- Badges -->
<p>

![Status: Mainnet Beta](https://img.shields.io/badge/Status-Mainnet_Beta-orange?style=for-the-badge)
![Protocol](https://img.shields.io/badge/Protocol-3.0.4-blue?style=for-the-badge)
![License: MIT](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)
![Rust](https://img.shields.io/badge/Rust-2021-orange?style=for-the-badge)
![PoW](https://img.shields.io/badge/Consensus-PoW-purple?style=for-the-badge)

</p>

<!-- Links -->
<p>

[🌐 Web](https://www.zionterranova.com)
&nbsp;·&nbsp;
[📖 Whitepaper](../../docs/whitepaper.md)
&nbsp;·&nbsp;
[🎮 Oasis](../../V3/L4/oasis/README.md)
&nbsp;·&nbsp;
[⚡ CLI](../../V3/cli/README.md)
&nbsp;·&nbsp;
[🔒 Security](../../SECURITY.md)

</p>

</div>

---

<div align="center">

## Las cuatro capas

</div>

| Capa | Nombre | Qué hace |
|:-----:|:----:|:---------|
| **L1** | **Core** | Blockchain PoW — la base. Algoritmo propio `deeksha_lite_v1`, bloques de 60s, minería CPU + GPU. |
| **L2** | **DeFi** | Staking, farming, gobernanza, puente cross-chain a 6 cadenas EVM (Base, Arbitrum, BSC, Polygon, Optimism, Avalanche). |
| **L3** | **WARP** | Router cross-chain — 21 adaptadores de cadena registrados, atomic swaps, capa de inferencia Hiran AI. |
| **L4** | **Oasis** | MMORPG espiritual de minería de consciencia — 199 avatares, 245 misiones, Golden Egg (108 pistas), 1B ZION en premios. |

<div align="center">

*ZION es una blockchain multicapa: núcleo L1 PoW, L2 DeFi y puente cross-chain, L3 WARP y Hiran AI, y L4 Oasis — un MMORPG espiritual de minería de consciencia.*

*Este repositorio contiene el código base de la red principal v3. Actualmente está en **Mainnet Beta**: activa, produciendo bloques, y abierta a minería bajo tu propio riesgo.*

</div>

---

<div align="center">

## Entra en el Oasis

</div>

| Portal | Camino |
|:------:|:-------|
| ⛏️ **Minar** | Ejecuta un nodo o minero en el ZION L1. Empieza con [`V3/cli/README.md`](../../V3/cli/README.md). |
| 🎮 **Jugar** | Entra al mundo de L4 Oasis — avatares, misiones, gremios y el Golden Egg. Ver [`V3/L4/oasis/README.md`](../../V3/L4/oasis/README.md). |
| 🔨 **Construir** | Explora el código, contratos, RPC y documentación del bridge en [`V3/docs/`](../../V3/docs/) y [`docs/`](../../docs/). |

---

<div align="center">

## Estado de la red

</div>

> **⚠️ Mainnet Beta — activo bajo tu propio riesgo**

| Parámetro | Valor |
|:----------|:------|
| **Estado** | Mainnet Beta |
| **Protocolo** | 3.0.4 |
| **Genesis hash** | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| **Lanzamiento oficial** | 2026-12-31 |
| **Tiempo de bloque** | ~60 segundos |
| **Algoritmo de minería** | `deeksha_lite_v1` (CPU + GPU) |
| **Suministro total** | 144B ZION |
| **Premine** | 14 slots (founders, treasury, OASIS pool, liquidity) |

Todos los problemas de seguridad divulgados han sido remediados. Ver [Security](../../SECURITY.md) y el [informe de divulgación](../../docs/security/SECURITY_DISCLOSURE_2026-07.md).

---

## Guía para principiantes — Empieza desde cero

> ¿Nunca has usado una blockchain? Estás en el lugar correcto.
> Esta guía te lleva paso a paso por todo el proceso.
> Solo necesitas un ordenador con Linux, macOS o Windows (WSL).

### ¿Qué es ZION en un párrafo?

ZION es una **blockchain proof-of-work** (como Bitcoin, pero con un algoritmo de minería diferente). Tiene su propia moneda llamada **ZION**. Puedes **minar** ZION con tu CPU o GPU, **enviarlo** a otros, y eventualmente **jugar** en el mundo de Oasis para ganar más. La red está activa ahora mismo — puedes unirte hoy.

### Paso 0 — Instala Rust

ZION está escrito en Rust. Necesitas la herramienta Rust para compilarlo.

```bash
# Linux / macOS / WSL — instala Rust vía rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Verifica que funciona
rustc --version
cargo --version
```

> **Usuarios de Windows:** Instala [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install) primero, luego ejecuta los comandos anteriores dentro de WSL. Las compilaciones nativas de Windows están planificadas pero aún no soportadas.

### Paso 1 — Obtén el código

```bash
git clone https://github.com/Zion-TerraNova/v3-Mainnet.git
cd v3-Mainnet/V3
```

### Paso 2 — Compila todo

Esto compila el nodo, CLI y minero. Tarda 5–15 minutos la primera vez.

```bash
# Compila todos los binarios (nodo + CLI + minero + pool + bridge + DAO + oasis)
cargo build --release

# Los binarios principales que usarás:
#   target/release/zion          — el CLI (monedero, minería, control del nodo)
#   target/release/zion-node     — el nodo blockchain
#   target/release/zion-miner    — minero independiente
```

> ¿Quieres minería con GPU? Añade un feature flag:
> - NVIDIA CUDA: `cargo build --release --features gpu-cuda -p zion-miner`
> - AMD / OpenCL genérico: `cargo build --release --features gpu-opencl -p zion-miner`
> - Apple Silicon Metal: `cargo build --release --features gpu-metal -p zion-miner`

### Paso 3 — Crea tu monedero

Tu monedero guarda tus ZION. Es un archivo JSON protegido por una contraseña que eliges.

```bash
# Genera un nuevo monedero con una frase de recuperación de 24 palabras (mnemonic)
# ¡APUNTA las 24 palabras en papel y guárdalas seguras — son tu única copia de seguridad!
./target/release/zion wallet new --mnemonic --out my-wallet.json

# Comprueba la dirección de tu monedero (aquí van las recompensas de minería)
./target/release/zion wallet info --wallet my-wallet.json
```

> **¿Qué es una dirección de monedero?** Es como un número de cuenta bancaria pero público — empieza con `zion1...` y puedes compartirla libremente. El mnemonic de 24 palabras es tu clave **privada** — nunca la compartas con nadie.

### Paso 4 — Ejecuta un nodo (opcional pero recomendado)

Un nodo se conecta a la red ZION, descarga la blockchain y verifica transacciones. Ejecutar uno ayuda a mantener la red descentralizada.

```bash
# Inicia el nodo (sincronizará la blockchain desde otros peers)
./target/release/zion-node

# En otro terminal, comprueba si funciona:
./target/release/zion node status
```

> **¿Qué es la sincronización?** El nodo descarga todos los bloques desde el bloque génesis hasta la punta actual. La primera vez puede tardar un rato. Después se mantiene actualizado automáticamente.

### Paso 5 — Empieza a minar

La minería es como se crea nuevo ZION. Tu ordenador resuelve puzzles matemáticos (proof-of-work), y cuando encuentra una solución, ganas una recompensa de bloque.

```bash
# La forma más fácil — ejecuta el asistente de configuración
./target/release/zion config init

# O empieza a minar directamente con tu monedero
./target/release/zion mine start --wallet my-wallet.json

# Comprueba el estado de la minería
./target/release/zion mine status

# Detén la minería
./target/release/zion mine stop
```

> **CPU vs GPU:** Minar con CPU funciona pero es lento. Una GPU (tarjeta gráfica) es mucho más rápida. Ejecuta `zion mine bench --gpu` para probar tu hashrate de GPU.
>
> **Pool vs Solo:** Por defecto, el CLI mina en el pool oficial (`pool.zionterranova.com:8444`). En modo pool, ganas una parte de cada bloque que el pool encuentra. En modo solo, solo ganas cuando *tú* encuentras un bloque — lo cual puede tardar mucho. El modo pool se recomienda para principiantes.

### Paso 6 — Comprueba tu saldo y envía ZION

```bash
# Comprueba tu saldo
./target/release/zion wallet balance --wallet my-wallet.json

# Envía ZION a alguien
./target/release/zion wallet send --to zion1... --amount 1.5 --wallet my-wallet.json
```

### Menú interactivo (lo más fácil para principiantes)

Si no quieres recordar comandos, simplemente ejecuta:

```bash
./target/release/zion menu
```

Se abre un menú interactivo con flechas — monedero, nodo, minería, pool y configuración.

### Glosario — términos clave explicados de forma sencilla

| Término | Qué significa |
|---------|--------------|
| **Blockchain** | Un libro mayor público de todas las transacciones, compartido entre muchos ordenadores |
| **Nodo** | Un ordenador ejecutando el software ZION que almacena y verifica la blockchain |
| **Minería** | Usar la potencia de tu ordenador para asegurar la red y ganar recompensas ZION |
| **Monedero** | Un archivo que guarda tus claves privadas — te permite enviar y recibir ZION |
| **Mnemonic** | 24 palabras que pueden restaurar tu monedero — anótalas, nunca las compartas |
| **Bloque** | Un grupo de transacciones añadido a la cadena cada ~60 segundos |
| **Pool** | Un grupo de mineros trabajando juntos — las recompensas se dividen entre participantes |
| **ZION** | La moneda de esta blockchain (ticker: ZION) |
| **Bloque génesis** | El primer bloque — la base de toda la cadena |
| **Mainnet Beta** | La red activa funciona pero puede tener errores — mina bajo tu propio riesgo |

### ¿Necesitas ayuda?

- **Documentación completa:** [README_FULL.es.md](./README_FULL.es.md)
- **Referencia CLI:** [`V3/cli/README.md`](../../V3/cli/README.md) — todos los comandos explicados
- **Documentos del nodo:** [`V3/docs/`](../../V3/docs/) — arquitectura, constantes, runbooks
- **Web:** [zionterranova.com](https://www.zionterranova.com)
- **Issues:** [GitHub Issues](https://github.com/Zion-TerraNova/v3-Mainnet/issues)

---

<div align="center">

## Idiomas

</div>

| | | | | |
|:---:|:---:|:---:|:---:|:---:|
| [English](../../README.md) | [Čeština](./README.cs.md) | **Español** | [Français](./README.fr.md) | [Português](./README.pt.md) |

---

<div align="center">

## Documentación completa

Para una visión completa de arquitectura, características, historia y hoja de ruta, ver **[README_FULL.es.md](./README_FULL.es.md)**.

</div>

---

<div align="center">

<img src="../../docs/stargate/Z.gif" width="48" height="48" alt="ZION" />

## Licencia

Este proyecto está licenciado bajo la [Licencia MIT](../../LICENSE).

---

### Construido con cuidado, asegurado por consenso.

[🌐 zionterranova.com](https://www.zionterranova.com) · [🔒 Security](../../SECURITY.md) · [📜 Whitepaper](../../docs/whitepaper.md) · [⚖️ Legal](../../docs/LEGAL_DISCLAIMER.md)

</div>
