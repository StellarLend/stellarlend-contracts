# Documentation Index

This document provides a central, canonical entry point for all project documentation.


## Core Documentation

### Security

- [Security Assumptions](SECURITY_ASSUMPTIONS.md)
- [Reentrancy Guarantees](REENTRANCY_GUARANTEES.md)

These documents define the protocol’s core security model, including assumptions, invariants, and protections against reentrancy and malicious interactions.


### Access Control & Admin

- [Admin and Access Control](admin.md)
- [Upgrade Authorization](UPGRADE_AUTHORIZATION.md)

Covers the `initialize` single-call guard, the `require_admin` helper, the two-step admin rotation pattern, the guardian role, and the auth boundaries for all privileged entrypoints.


### Integration

- [Interface Quick Reference](interface_quick_reference.md)

Single-page reference for frontend integrators: unit scales, entrypoint signatures, error code mappings, and integration checklist.


### Lending & Risk

- [Risk Parameters](risk_params.md)

Covers protocol-level parameters such as collateral ratios, liquidation thresholds, and limits enforced by the lending system.


### Operations & Deployment

- [Deployment Guide](deployment.md)

Provides step-by-step instructions for building, deploying, and initializing contracts on testnet and mainnet.


### Interface & API

- [Interface Quick Reference](interface_quick_reference.md)
- [Lending Contract README](../stellar-lend/contracts/lending/README.md)

Documents the **shipping** function surface of the lending contract (`src/lib.rs`), including exact signatures, return types, error codes, and emergency state permissions. Functions that are planned but not yet implemented are explicitly marked as 🔮 Planned.


## Additional Documentation

- Threat models and extended security analysis (if present)
- Integration or architecture-specific notes


## Historical Documents

⚠️ The following documents are retained for reference but may be outdated or superseded:

- PR summaries
- Temporary notes
- Early drafts or exploratory documentation

These should not be considered the source of truth for the current system.


## Source of Truth

For accuracy and implementation details, always prefer:

- Structured documentation in `/docs`
- Contract source code in `stellar-lend/contracts/lending/src/`

*Consolidation Rationale*: Historically, a separate and misnamed source tree (`contracts/lending/scr`) existed and caused workspace resolution confusion. We have consolidated all active, salvageable code (like the `rounding_strategy`) into the canonical workspace crate at `stellar-lend/contracts/lending/src/` and deleted the stray tree. This ensures a single source of truth for the lending protocol and proper Cargo workspace builds.

## Rationale

This index improves discoverability and ensures contributors can quickly locate authoritative documentation without confusion from scattered or outdated files.
