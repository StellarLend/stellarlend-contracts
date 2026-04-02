# Configuration Snapshot Module

## Overview

The `ConfigSnapshot` module provides off-chain tooling, frontends, and liquidators with a point-in-time, comprehensive view of the protocol's global metrics and risk parameters. It is accessed via the `get_config_snapshot` endpoint on the StellarLend core contract.

## Security Model and Read-Only Guarantees

- **State Indivisibility:** Creating a `ConfigSnapshot` is guaranteed to be a strictly read-only operation. Calls to `get_config_snapshot` only utilize `get` operations against persistent storage and perform zero state mutations.
- **No Token Movement:** The snapshot logic contains zero token transfer authorization paths. No balance states, vault configurations, or underlying ledger data can be altered by this endpoint.
- **Reentrancy Immunity:** There are zero cross-contract invocations involved in fetching the protocol configuration. Reentrancy is entirely mitigated by design for this procedure.

## Trust Boundaries and Interaction Flow

`ConfigSnapshot` aggregates data dictated by:
1. **Administrative Governance:** Parameters like the `min_collateral_ratio`, `close_factor`, `liquidation_threshold`, and `liquidation_incentive` can strictly only be modified by the designated `admin` or active governance `guardians`.
2. **Emergency Safeguards:** The `emergency_pause` switch is reflected here directly from emergency operational controls—which is an admin-guaranteed constraint.

## Authorization

**None.** Given its zero-side-effect profile, the `get_config_snapshot` endpoint requires *no authorization*. Any anonymous address or contract can securely request the configuration struct without requiring key material or signed invocations.
