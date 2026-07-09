# ZION

<div align="center">

<img src="../../docs/stargate/nebula.jpg" width="260" height="260" alt="ZION Stargate" style="border-radius: 50%; object-fit: cover; box-shadow: 0 0 50px rgba(0,180,255,0.25);" />

<br/>

## Terra Nova — 100 anos de evoluZion

**Um ecossistema Dharma multichain protegido por consenso proof-of-work.**

[www.zionterranova.com](https://www.zionterranova.com)

<br/>

</div>

ZION é uma blockchain de múltiplas camadas: núcleo PoW L1, DeFi L2 e ponte cross-chain, WARP L3 e Hiran AI, e Oasis L4 — um MMORPG espiritual de mineração da consciência.

Este repositório contém a base de código da rede principal v3. Atualmente está em **Mainnet Beta**: ativa, produzindo blocos e aberta à mineração por sua própria conta e risco.

---

## Entre no Oasis

| Portal | Caminho |
|---|---|
| **Minerar** | Execute um nó ou minerador na ZION L1. Comece em [`V3/cli/README.md`](../../V3/cli/README.md). |
| **Jogar** | Entre no mundo L4 Oasis — avatares, missões, guildas e o Golden Egg. Veja [`V3/L4/oasis/README.md`](../../V3/L4/oasis/README.md). |
| **Construir** | Explore o código, contratos, RPC e documentação da ponte em [`V3/docs/`](../../V3/docs/) e [`docs/`](../../docs/). |

---

## Status da rede

> **Mainnet Beta — ativa por sua própria conta e risco**

| Parâmetro | Valor |
|---|---|
| Status | Mainnet Beta |
| Protocolo | 3.0.4 |
| Hash do gênesis | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Lançamento oficial | 2026-12-31 |

Todos os problemas de segurança divulgados foram remediados. Veja [Security](../../SECURITY.md) e o [relatório de divulgação](../../docs/security/SECURITY_DISCLOSURE_2026-07.md).

---

## Guia para iniciantes — Comece do zero

> Nunca usou uma blockchain? Está no lugar certo.
> Este guia leva você passo a passo por todo o processo.
> Só precisa de um computador com Linux, macOS ou Windows (WSL).

### O que é ZION num parágrafo?

ZION é uma **blockchain proof-of-work** (como o Bitcoin, mas com um algoritmo de mineração diferente). Tem a sua própria moeda chamada **ZION**. Pode **minar** ZION com o seu CPU ou GPU, **enviá-lo** a outros, e eventualmente **jogar** no mundo de Oasis para ganhar mais. A rede está ativa agora mesmo — pode juntar-se hoje.

### Passo 0 — Instale Rust

ZION está escrito em Rust. Precisa da cadeia de ferramentas Rust para o compilar.

```bash
# Linux / macOS / WSL — instale Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Verifique que funciona
rustc --version
cargo --version
```

> **Utilizadores de Windows:** Instale primeiro o [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install), depois execute os comandos acima dentro do WSL. As compilações nativas de Windows estão planeadas mas ainda não suportadas.

### Passo 1 — Obtenha o código

```bash
git clone https://github.com/Zion-TerraNova/v3-Mainnet.git
cd v3-Mainnet/V3
```

### Passo 2 — Compile tudo

Isto compila o nó, o CLI e o minerador. Demora 5 a 15 minutos na primeira vez.

```bash
# Compile todos os binários (nó + CLI + minerador + pool + bridge + DAO + oasis)
cargo build --release

# Os principais binários que vai usar:
#   target/release/zion          — o CLI (carteira, mineração, controlo do nó)
#   target/release/zion-node     — o nó blockchain
#   target/release/zion-miner    — minerador autónomo
```

> **Quer mineração com GPU?** Adicione um feature flag:
> - NVIDIA CUDA: `cargo build --release --features gpu-cuda -p zion-miner`
> - AMD / OpenCL genérico: `cargo build --release --features gpu-opencl -p zion-miner`
> - Apple Silicon Metal: `cargo build --release --features gpu-metal -p zion-miner`

### Passo 3 — Crie a sua carteira

A sua carteira guarda os seus ZION. É um ficheiro JSON protegido por uma palavra-passe que escolhe.

```bash
# Gere uma nova carteira com uma frase de recuperação de 24 palavras (mnemonic)
# APONTE as 24 palavras em papel e guarde-as em segurança — são a sua única cópia de segurança!
./target/release/zion wallet new --mnemonic --out my-wallet.json

# Verifique o endereço da sua carteira (é para aqui que vão as recompensas de mineração)
./target/release/zion wallet info --wallet my-wallet.json
```

> **O que é um endereço de carteira?** É como um número de conta bancária, mas público — começa com `zion1...` e pode partilhá-lo livremente. O mnemonic de 24 palavras é a sua chave **privada** — nunca a partilhe com ninguém.

### Passo 4 — Execute um nó (opcional mas recomendado)

Um nó liga-se à rede ZION, descarrega a blockchain e verifica transações. Executar um nó ajuda a manter a rede descentralizada.

```bash
# Inicie o nó (vai sincronizar a blockchain a partir de outros peers)
./target/release/zion-node

# Noutro terminal, verifique se está a funcionar:
./target/release/zion node status
```

> **O que é a sincronização?** O nó descarrega todos os blocos desde o bloco génese até à ponta atual. Na primeira execução pode demorar algum tempo. Depois disso, mantém-se atualizado automaticamente.

### Passo 5 — Comece a minar

A mineração é a forma como novos ZION são criados. O seu computador resolve puzzles matemáticos (proof-of-work), e quando encontra uma solução, ganha uma recompensa de bloco.

```bash
# A forma mais fácil — execute o assistente de configuração
./target/release/zion config init

# Ou comece a minar diretamente com a sua carteira
./target/release/zion mine start --wallet my-wallet.json

# Verifique o estado da mineração
./target/release/zion mine status

# Pare a mineração
./target/release/zion mine stop
```

> **CPU vs GPU:** Minar com CPU funciona mas é lento. Uma GPU (placa gráfica) é muito mais rápida. Execute `zion mine bench --gpu` para testar o hashrate da sua GPU.
>
> **Pool vs Solo:** Por padrão, o CLI mina no pool oficial (`pool.zionterranova.com:8444`). No modo pool, ganha uma parte de cada bloco que o pool encontra. No modo solo, só ganha quando *você* encontra um bloco — o que pode demorar muito tempo. O modo pool é recomendado para iniciantes.

### Passo 6 — Verifique o seu saldo e envie ZION

```bash
# Verifique o seu saldo
./target/release/zion wallet balance --wallet my-wallet.json

# Envie ZION a alguém
./target/release/zion wallet send --to zion1... --amount 1.5 --wallet my-wallet.json
```

### Menu interativo (o mais fácil para iniciantes)

Se não quer memorizar comandos, simplesmente execute:

```bash
./target/release/zion menu
```

Abre-se um menu interativo com setas — carteira, nó, mineração, pool e configuração.

### Glossário — termos-chave explicados de forma simples

| Termo | O que significa |
|-------|----------------|
| **Blockchain** | Um livro-razão público de todas as transações, partilhado entre muitos computadores |
| **Nó** | Um computador a executar o software ZION que armazena e verifica a blockchain |
| **Mineração** | Usar o poder do seu computador para proteger a rede e ganhar recompensas ZION |
| **Carteira** | Um ficheiro que guarda as suas chaves privadas — permite-lhe enviar e receber ZION |
| **Mnemonic** | 24 palavras que podem restaurar a sua carteira — anote-as, nunca as partilhe |
| **Bloco** | Um grupo de transações adicionado à cadeia a cada ~60 segundos |
| **Pool** | Um grupo de mineradores a trabalhar juntos — as recompensas são divididas entre os participantes |
| **ZION** | A moeda desta blockchain (ticker: ZION) |
| **Bloco génese** | O primeiro bloco — a fundação de toda a cadeia |
| **Mainnet Beta** | A rede ativa funciona mas pode ainda ter bugs — mine por sua conta e risco |

### Precisa de ajuda?

- **Documentação completa:** [README_FULL.pt.md](./README_FULL.pt.md)
- **Referência CLI:** [`V3/cli/README.md`](../../V3/cli/README.md) — todos os comandos explicados
- **Documentos do nó:** [`V3/docs/`](../../V3/docs/) — arquitetura, constantes, runbooks
- **Website:** [zionterranova.com](https://www.zionterranova.com)
- **Issues:** [GitHub Issues](https://github.com/Zion-TerraNova/v3-Mainnet/issues)

---

## Idiomas

[English](../../README.md) · [Čeština](./README.cs.md) · [Español](./README.es.md) · [Français](./README.fr.md) · **Português**

---

## Documentação completa

Para uma visão completa da arquitetura, recursos, histórico e roteiro, consulte **[README_FULL.pt.md](./README_FULL.pt.md)**.

---

## Licença

Este projeto está licenciado sob a [Licença MIT](../../LICENSE).

<div align="center">

Construído com cuidado, protegido por consenso.

</div>
