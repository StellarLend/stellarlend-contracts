# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in StellarLend smart contracts, the oracle service, or the API, **please do not open a public GitHub issue or pull request**.

Instead, report it privately using GitHub Private Vulnerability Reporting:

- **Private Advisory**: [Open a private security advisory](https://github.com/StellarLend/stellarlend-contracts/security/advisories/new)

### What to Include

To help maintainers triage and reproduce the issue quickly, please include:

- A clear description of the vulnerability and its potential impact.
- The affected component(s), contract entrypoint(s), or file path(s).
- Step-by-step reproduction instructions or a minimal proof-of-concept test.
- Any suggested mitigations or fixes (optional).

### Response Process

- Maintainers will acknowledge receipt of your report as soon as possible and coordinate a private fix and disclosure timeline with you.
- Please allow maintainers reasonable time to investigate, patch, and deploy mitigations before any public disclosure.

## Scope & Related Security Documentation

- **Incident Response & Pause Runbook**: See [docs/INCIDENT_RESPONSE.md](docs/INCIDENT_RESPONSE.md) for emergency pause switches, read-only mode, and bridge freeze procedures.
- **Lending Contract Security Notes**: See [stellar-lend/contracts/lending/SECURITY.md](stellar-lend/contracts/lending/SECURITY.md) for contract-level trust boundaries, authorization invariants, liquidation invariants, oracle signature verification, and reentrancy protections.
