# Contributing to ZION v3

Thank you for your interest in contributing to ZION! This document outlines the process for contributing to the ZION v3 blockchain infrastructure.

## Getting Started

### Prerequisites

- **Rust** (stable, latest stable toolchain)
- **Foundry** (for Solidity contracts — `forge`, `cast`, `anvil`)
- **Node.js** 18+ (for Hardhat scripts)
- **Git**

### Building from Source

```bash
git clone https://github.com/Zion-TerraNova/v3-Mainnet.git
cd v3-Mainnet
cargo build --release
```

### Running Tests

```bash
# L1 core tests
cargo test -p zion-core

# L2 bridge tests
cargo test -p zion-bridge

# L2 DAO tests
cargo test -p zion-dao

# All tests
cargo test --workspace

# Solidity contracts
cd V3/L2/contracts && forge test
```

## Development Workflow

### 1. Fork & Branch

```bash
# Fork the repo on GitHub, then:
git clone https://github.com/<your-username>/v3-Mainnet.git
cd v3-Mainnet
git remote add upstream https://github.com/Zion-TerraNova/v3-Mainnet.git
git checkout -b feature/your-feature-name
```

### 2. Code Style

- **Rust:** Follow `rustfmt` defaults. Run `cargo fmt` before committing.
- **Solidity:** Follow [Solcurity Standard](https://github.com/Rari-Capital/solcurity) guidelines.
- **Commits:** Use [Conventional Commits](https://www.conventionalcommits.org/) format:
  - `feat: add new RPC endpoint`
  - `fix: correct balance validation in peer block`
  - `docs: update consensus documentation`
  - `refactor: simplify merkle root computation`

### 3. Testing Requirements

All PRs must include:
- **Unit tests** for new functionality
- **Integration tests** for consensus changes
- **No regressions** — existing tests must pass
- **`cargo clippy`** must pass without warnings

### 4. Pull Request Process

1. **Update documentation** if your change affects public APIs or consensus behavior
2. **Add tests** for new features
3. **Run `cargo fmt && cargo clippy && cargo test`** before submitting
4. **Reference related issues** in your PR description
5. **Describe the "why"** — not just the "what"

### PR Template

```markdown
## Description
[What does this PR do and why?]

## Type of Change
- [ ] Bug fix (non-breaking)
- [ ] New feature (non-breaking)
- [ ] Breaking change (requires hard fork)
- [ ] Documentation update
- [ ] Security fix

## Testing
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes
- [ ] `cargo fmt` passes
- [ ] New tests added

## Consensus Impact
- [ ] No consensus change
- [ ] Consensus change (requires coordinated upgrade)
```

## L1 Consensus Changes

**L1 consensus changes are treated with the highest scrutiny.** Any change that affects:
- Transaction validation
- Block validation
- Signature verification
- Genesis configuration
- Fee structure
- Emission schedule

...requires:
1. **Detailed specification** of the change and its rationale
2. **Height-gated activation** (not immediate — follows the pattern of `TX_HASH_V2`, `ACCOUNT_TX_MEMO_V1`)
3. **Testnet deployment** and verification
4. **Coordinated upgrade** of all network participants
5. **Security review** by at least one core contributor

## Security Vulnerabilities

**Do NOT open a PR or public issue for security vulnerabilities.** See [SECURITY.md](./SECURITY.md) for responsible disclosure process.

## Code of Conduct

All contributors are expected to adhere to our [Code of Conduct](./CODE_OF_CONDUCT.md). Be respectful, constructive, and welcoming.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](./LICENSE).
