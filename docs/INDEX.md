# Documentation Index

This document provides a central, canonical entry point for all project documentation.

> **Contributing docs**: All canonical documentation belongs under `docs/`.  
> Do not add markdown files to the repository root — they are not discoverable here
> and will be removed. If you need to attach notes to a PR or issue, use a GitHub
> comment or a `docs/` subdirectory. Scratch files (`*_SUMMARY.md`, `PR_*.md`,
> `*_output.txt`, etc.) are periodically pruned from root.

---

## Core Documentation

### Security

- [Security Assumptions](SECURITY_ASSUMPTIONS.md)
- [Pause Mechanism Security Analysis](PAUSE_SECURITY_ANALYSIS.md)
- [Threat Model: Flash Loans & Token Receiver](THREAT_MODEL_FLASHLOAN_TOKEN_RECEIVER.md)
- [Reentrancy Guarantees](../stellar-lend/docs/REENTRANCY_GUARANTEES.md)

These documents define the protocol's core security model, including assumptions, invariants, and protections against reentrancy and malicious interactions.

### Access Control & Admin

- [Admin and Access Control](admin.md)
- [Upgrade Authorization](UPGRADE_AUTHORIZATION.md)
- [Multisig Governance](multisig.md)
- [Timelock Governance](timelock-governance.md)

Covers the `initialize` single-call guard, the `require_admin` helper, the two-step admin rotation pattern, the guardian role, and the auth boundaries for all privileged entrypoints.

### Lending & Risk

- [Risk Parameters](risk_params.md)
- [Reserve Accounting](RESERVE_ACCOUNTING.md)
- [Protocol Accounting](PROTOCOL_ACCOUNTING.md)
- [Cross-Asset Rules](CROSS_ASSET_RULES.md)
- [Zero Amount Semantics](ZERO_AMOUNT_SEMANTICS.md)
- [View Schema Versioning Policy](VIEW_SCHEMA_VERSIONING_POLICY.md)

Covers protocol-level parameters such as collateral ratios, liquidation thresholds, and limits enforced by the lending system.

### Integration

- [Interface Quick Reference](interface_quick_reference.md)
- [Developer Glossary](glossary.md)

Single-page reference for frontend integrators: unit scales, entrypoint signatures, error code mappings, and integration checklist.

### Operations & Deployment

- [Deployment Guide](deployment.md)
- [Release Checklist](release_checklist.md)
- [Incident Response](INCIDENT_RESPONSE.md)
- [Upgrade Playbook](upgrade_playbook.md)
- [Recovery Guide](recovery.md)

Provides step-by-step instructions for building, deploying, and initializing contracts on testnet and mainnet.

### Testing & CI

- [Borrow Tests](BORROW_TESTS.md)
- [Initialization Tests](INITIALIZATION_TESTS.md)
- [Local CI Runbook](LOCAL_CI_RUNBOOK.md)
- [CI/CD Overview](CI_OVERVIEW.md)

Test documentation and CI pipeline reference.

### Architecture & Reference

- [Project Summary](PROJECT_SUMMARY.md)
- [Event Schema Versioning](EVENT_SCHEMA_VERSIONING.md)
- [Activity Ordering Guarantees](ACTIVITY_ORDERING_GUARANTEES.md)
- [Oracle Configuration Guide](ORACLE_CONFIGURATION_GUIDE.md)
- [Reserve](reserve.md)
- [Storage Layout](storage.md)
- [Storage Tier Reference](STORAGE_TIER_REFERENCE.md)
- [Admin Operations](admin.md)

---

## External Canonical Docs

These documents live alongside the contract source and are the authoritative reference for their respective domains:

- [Lending Contract README](../stellar-lend/contracts/lending/README.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/borrow.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/deposit.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/pause.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/flash_loan.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/cross_asset.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/views.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/token_receiver.md)
- [Lending Contract Docs](../stellar-lend/contracts/lending/emergency_shutdown.md)
- [Repay Semantics](../stellar-lend/docs/REPAY_SEMANTICS.md)
- [Reentrancy Guarantees](../stellar-lend/docs/REENTRANCY_GUARANTEES.md)
- [Errors](../stellar-lend/docs/ERRORS.md)

---

## Source of Truth

For accuracy and implementation details, always prefer:

- Structured documentation in `docs/`
- Contract source code in `stellar-lend/contracts/lending/src/`
- Component docs under `stellar-lend/contracts/lending/`

*Consolidation Rationale*: Historically, the repository root accumulated PR artifacts,
scratch files, and one-off summaries that obscured canonical documentation. All such
files have been removed or migrated under `docs/`. If you find a root-level `.md`
file that isn't `README.md` or `CONTRIBUTORS.md`, it is likely stale — open an issue
to have it migrated or removed.
