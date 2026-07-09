# ZION

<div align="center">

<img src="../../docs/stargate/nebula.jpg" width="260" height="260" alt="ZION Stargate" style="border-radius: 50%; object-fit: cover; box-shadow: 0 0 50px rgba(0,180,255,0.25);" />

<br/>

## Terra Nova — 100 ans d'evoluZion

**Un écosystème Dharma multichain sécurisé par consensus proof-of-work.**

[www.zionterranova.com](https://www.zionterranova.com)

<br/>

</div>

ZION est une blockchain multicouche : cœur PoW L1, DeFi L2 et pont cross-chain, WARP L3 et Hiran AI, et Oasis L4 — un MMORPG spirituel de minage de la conscience.

Ce dépôt contient le code source du réseau principal v3. Il est actuellement en **Mainnet Beta** : actif, produisant des blocs et ouvert au minage à vos propres risques.

---

## Entrez dans l'Oasis

| Portail | Chemin |
|---|---|
| **Miner** | Exécutez un nœud ou un mineur sur ZION L1. Commencez avec [`V3/cli/README.md`](../../V3/cli/README.md). |
| **Jouer** | Entrez dans le monde L4 Oasis — avatars, quêtes, guildes et le Golden Egg. Voir [`V3/L4/oasis/README.md`](../../V3/L4/oasis/README.md). |
| **Construire** | Explorez le code, les contrats, le RPC et la documentation du pont dans [`V3/docs/`](../../V3/docs/) et [`docs/`](../../docs/). |

---

## Statut du réseau

> **Mainnet Beta — actif à vos propres risques**

| Paramètre | Valeur |
|---|---|
| Statut | Mainnet Beta |
| Protocole | 3.0.4 |
| Hash du genesis | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Lancement officiel | 2026-12-31 |

Tous les problèmes de sécurité divulgués ont été remédiés. Voir [Security](../../SECURITY.md) et le [rapport de divulgation](../../docs/security/SECURITY_DISCLOSURE_2026-07.md).

---

## Guide du débutant — Commencez de zéro

> Vous n'avez jamais utilisé de blockchain ? Vous êtes au bon endroit.
> Ce guide vous accompagne pas à pas dans tout le processus.
> Il vous faut juste un ordinateur sous Linux, macOS ou Windows (WSL).

### Qu'est-ce que ZION en un paragraphe ?

ZION est une **blockchain proof-of-work** (comme Bitcoin, mais avec un algorithme de minage différent). Elle a sa propre monnaie appelée **ZION**. Vous pouvez **miner** du ZION avec votre CPU ou GPU, l'**envoyer** à d'autres, et éventuellement **jouer** dans le monde d'Oasis pour en gagner davantage. Le réseau est actif dès maintenant — vous pouvez le rejoindre aujourd'hui.

### Étape 0 — Installez Rust

ZION est écrit en Rust. Vous avez besoin de la chaîne d'outils Rust pour le compiler.

```bash
# Linux / macOS / WSL — installez Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Vérifiez que ça fonctionne
rustc --version
cargo --version
```

> **Utilisateurs Windows :** Installez d'abord [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install), puis exécutez les commandes ci-dessus dans WSL. Les compilations natives Windows sont prévues mais pas encore prises en charge.

### Étape 1 — Obtenez le code

```bash
git clone https://github.com/Zion-TerraNova/v3-Mainnet.git
cd v3-Mainnet/V3
```

### Étape 2 — Compilez tout

Cela compile le nœud, le CLI et le mineur. Cela prend 5 à 15 minutes la première fois.

```bash
# Compilez tous les binaires (nœud + CLI + mineur + pool + bridge + DAO + oasis)
cargo build --release

# Les principaux binaires que vous utiliserez :
#   target/release/zion          — le CLI (portefeuille, minage, contrôle du nœud)
#   target/release/zion-node     — le nœud blockchain
#   target/release/zion-miner    — mineur autonome
```

> **Vous voulez miner avec GPU ?** Ajoutez un feature flag :
> - NVIDIA CUDA : `cargo build --release --features gpu-cuda -p zion-miner`
> - AMD / OpenCL générique : `cargo build --release --features gpu-opencl -p zion-miner`
> - Apple Silicon Metal : `cargo build --release --features gpu-metal -p zion-miner`

### Étape 3 — Créez votre portefeuille

Votre portefeuille contient vos ZION. C'est un fichier JSON protégé par un mot de passe que vous choisissez.

```bash
# Générez un nouveau portefeuille avec une phrase de récupération de 24 mots (mnemonic)
# NOTEZ les 24 mots sur papier et gardez-les en sécurité — c'est votre seule sauvegarde !
./target/release/zion wallet new --mnemonic --out my-wallet.json

# Vérifiez l'adresse de votre portefeuille (c'est là que vont les récompenses de minage)
./target/release/zion wallet info --wallet my-wallet.json
```

> **Qu'est-ce qu'une adresse de portefeuille ?** C'est comme un numéro de compte bancaire mais public — elle commence par `zion1...` et vous pouvez la partager librement. Le mnemonic de 24 mots est votre clé **privée** — ne la partagez jamais avec personne.

### Étape 4 — Lancez un nœud (optionnel mais recommandé)

Un nœud se connecte au réseau ZION, télécharge la blockchain et vérifie les transactions. En faire fonctionner un aide à maintenir le réseau décentralisé.

```bash
# Démarrez le nœud (il synchronisera la blockchain depuis d'autres pairs)
./target/release/zion-node

# Dans un autre terminal, vérifiez s'il fonctionne :
./target/release/zion node status
```

> **Qu'est-ce que la synchronisation ?** Le nœud télécharge tous les blocs depuis le bloc genèse jusqu'à la pointe actuelle. Cela peut prendre un certain temps au premier lancement. Ensuite, il se maintient à jour automatiquement.

### Étape 5 — Commencez à miner

Le minage est la façon dont de nouveaux ZION sont créés. Votre ordinateur résout des puzzles mathématiques (proof-of-work), et quand il trouve une solution, vous gagnez une récompense de bloc.

```bash
# Le plus simple — lancez l'assistant de configuration
./target/release/zion config init

# Ou commencez à miner directement avec votre portefeuille
./target/release/zion mine start --wallet my-wallet.json

# Vérifiez l'état du minage
./target/release/zion mine status

# Arrêtez le minage
./target/release/zion mine stop
```

> **CPU vs GPU :** Miner avec un CPU fonctionne mais c'est lent. Un GPU (carte graphique) est beaucoup plus rapide. Exécutez `zion mine bench --gpu` pour tester votre hashrate GPU.
>
> **Pool vs Solo :** Par défaut, le CLI mine vers le pool officiel (`pool.zionterranova.com:8444`). En mode pool, vous gagnez une part de chaque bloc que le pool trouve. En mode solo, vous ne gagnez que lorsque *vous* trouvez un bloc — ce qui peut prendre beaucoup de temps. Le mode pool est recommandé pour les débutants.

### Étape 6 — Vérifiez votre solde et envoyez du ZION

```bash
# Vérifiez votre solde
./target/release/zion wallet balance --wallet my-wallet.json

# Envoyez du ZION à quelqu'un
./target/release/zion wallet send --to zion1... --amount 1.5 --wallet my-wallet.json
```

### Menu interactif (le plus simple pour les débutants)

Si vous ne voulez pas mémoriser de commandes, lancez simplement :

```bash
./target/release/zion menu
```

Cela ouvre un menu interactif avec flèches — portefeuille, nœud, minage, pool et configuration.

### Glossaire — termes clés expliqués simplement

| Terme | Ce que ça veut dire |
|-------|---------------------|
| **Blockchain** | Un grand livre public de toutes les transactions, partagé entre de nombreux ordinateurs |
| **Nœud** | Un ordinateur exécutant le logiciel ZION qui stocke et vérifie la blockchain |
| **Minage** | Utiliser la puissance de votre ordinateur pour sécuriser le réseau et gagner des récompenses ZION |
| **Portefeuille** | Un fichier qui contient vos clés privées — il vous permet d'envoyer et de recevoir du ZION |
| **Mnemonic** | 24 mots qui peuvent restaurer votre portefeuille — notez-les, ne les partagez jamais |
| **Bloc** | Un groupe de transactions ajouté à la chaîne toutes les ~60 secondes |
| **Pool** | Un groupe de mineurs travaillant ensemble — les récompenses sont réparties entre les participants |
| **ZION** | La monnaie de cette blockchain (ticker : ZION) |
| **Bloc genèse** | Le tout premier bloc — la fondation de toute la chaîne |
| **Mainnet Beta** | Le réseau actif fonctionne mais peut encore avoir des bugs — minez à vos propres risques |

### Besoin d'aide ?

- **Documentation complète :** [README_FULL.fr.md](./README_FULL.fr.md)
- **Référence CLI :** [`V3/cli/README.md`](../../V3/cli/README.md) — toutes les commandes expliquées
- **Documents du nœud :** [`V3/docs/`](../../V3/docs/) — architecture, constantes, runbooks
- **Site web :** [zionterranova.com](https://www.zionterranova.com)
- **Issues :** [GitHub Issues](https://github.com/Zion-TerraNova/v3-Mainnet/issues)

---

## Langues

[English](../../README.md) · [Čeština](./README.cs.md) · [Español](./README.es.md) · **Français** · [Português](./README.pt.md)

---

## Documentation complète

Pour une vue d'ensemble complète de l'architecture, des fonctionnalités, de l'historique et de la feuille de route, consultez **[README_FULL.fr.md](./README_FULL.fr.md)**.

---

## Licence

Ce projet est sous licence [MIT](../../LICENSE).

<div align="center">

Construit avec soin, sécurisé par consensus.

</div>
