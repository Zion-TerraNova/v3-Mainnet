# Security Policy

## Reporting a Vulnerability

**DO NOT open a public GitHub issue** to report a security vulnerability.

### How to Report

| Channel | When to Use |
|---------|-------------|
| **GitHub Security Advisories** (preferred) | [github.com/Zion-TerraNova/v3-Mainnet/security/advisories](https://github.com/Zion-TerraNova/v3-Mainnet/security/advisories) — use "Report a vulnerability" |
| **Email** | `security@zionterranova.com` — include your GitHub username so we can add you to a private advisory |

**Do NOT include** exploit details or proof-of-concept in the initial email. Wait for a private advisory to be created, then share sensitive details there.

### What to Include in Your Report

- A clear description of the vulnerability and affected component
- Steps to reproduce (Foundry test, script, or transaction hash on testnet)
- Your assessment of severity (see rubric below)
- A way to contact you for follow-up questions
- Your preferred attribution (handle, name, or anonymous)

## Response Timeline

| Stage | Target |
|-------|--------|
| Acknowledgment | Within 48 hours |
| Initial assessment | Within 7 days |
| Fix development | Within 30 days (severity-dependent) |
| Coordinated disclosure | After fix is deployed, timeline agreed with reporter |

We commit to:
- Not pursuing legal action against researchers acting in good faith
- Coordinating disclosure timeline with the reporter
- Crediting the reporter (if desired) in the security advisory

## Scope

### In Scope

| Component | Path |
|-----------|------|
| L1 consensus core | `V3/L1/core/` |
| L1 mining pool | `V3/L1/pool/` |
| L1 miner | `V3/L1/miner/` |
| L2 smart contracts | `V3/L2/contracts/` |
| L2 bridge relay | `V3/L2/bridge/` |
| L2 DAO governance | `V3/L2/dao/` |
| L2 atomic swap | `V3/L2/atomic-swap/` |

### Out of Scope

- Third-party services (SimpleMining, hosting providers, etc.)
- Frontend / web application (separate repository)
- Infrastructure / deployment configuration
- Social engineering attacks
- Attacks requiring physical access to validator hardware

## Severity Classification

| Severity | Impact | Example |
|----------|--------|---------|
| **Critical** | Fund loss, consensus break, unauthorized minting | Ability to forge transactions or mint tokens |
| **High** | Significant fund risk, DoS of network | Remote crash of all nodes, balance bypass |
| **Medium** | Limited fund risk, degraded service | RPC leak of sensitive data, local DoS |
| **Low** | Minimal impact | Information disclosure without fund risk |

## Known Vulnerabilities (Disclosed)

All previously identified vulnerabilities have been remediated. See:
- [`docs/security/SECURITY_DISCLOSURE_2026-07.md`](docs/security/SECURITY_DISCLOSURE_2026-07.md) — ZION-2026-001 through ZION-2026-005
- [`docs/security/vulnerabilities.json`](docs/security/vulnerabilities.json) — Machine-readable vulnerability data

| ID | Title | Status |
|----|-------|--------|
| ZION-2026-001 | Forged P2P account TX signatures (F1) | ✅ Fixed |
| ZION-2026-002 | Unlimited inflation via account model (F5) | ✅ Fixed |
| ZION-2026-003 | Server exposure (C1-C8) | ✅ Fixed |
| ZION-2026-004 | TeamViewer compromise | ✅ Fixed |
| ZION-2026-005 | EVM key compromise | ✅ Fixed |

## Supported Versions

| Version | Supported | Status |
|---------|-----------|--------|
| 3.0.4 | ✅ | Current mainnet |
| < 3.0.4 | ❌ | Not supported — upgrade required |

## Security Best Practices for Node Operators

1. **Never commit private keys** to any repository (public or private)
2. **Use environment variables** for all sensitive configuration
3. **Run services on localhost** (127.0.0.1) — do not expose RPC to public internet
4. **Use SSH key-only authentication** — disable password login
5. **Enable firewall** (UFW) — allow only P2P + SSH + HTTP/HTTPS
6. **Regular security updates** — keep OS and dependencies patched
7. **Air-gapped key management** — generate and store genesis/premine keys offline
