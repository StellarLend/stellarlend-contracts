# Multisig Security Assumptions and Trust Boundaries

## Overview
This document outlines the security assumptions, trust boundaries, and execution guards for the StellarLend Multisig operations implemented within the governance module. The recent addition of expiry bounds and reentrancy protections ensures robust execution and prevents stale proposals from executing under shifted contexts.

## Trust Boundaries
1. **Multisig Admins:** Listed exclusively in `MultisigConfig`. They hold ultimate authorization over privileged operational parameter changes. They are fully trusted not to collude maliciously but are protected against individual compromises through a strict `threshold`.
2. **Executors:** Any authorized admin can execute an approved proposal. Execution power has no extra privilege beyond what is inscribed in the `Proposal` itself. Executors are not trusted; all operations strictly validate the exact `Proposal` contents.
3. **Guardians:** Authorized solely to trigger social recovery and rotate keys, not to craft operational parameter updates.

## Token Transfer Flows
- Proposals do not transfer voting tokens. They execute administrative changes through `execute_proposal_type`. The state change is strictly bounded to configuring protocol indices (e.g. `min_collateral_ratio`, pausing features, etc). 

## Proposal State and Lifecycle (Reentrancy and Guards)
1. **Checks-Effects-Interactions (CEI):** To prevent reentrancy during external action invocations (e.g., `GenericAction` calling an unknown smart contract), the proposal's state is modified explicitly to `ProposalStatus::Executed` **BEFORE** dispatching the underlying call.
   - If the call fails, the state is reliably rolled back to `Active`/`Queued` prior to returning an error cleanly. This strict pre-check ensures that nested executions will encounter the `ProposalAlreadyExecuted` failure guard.
2. **Stale Execution Guard:** `start_time`, `execution_delay` and `timelock_duration` implement an enforced expiration period (`expiry = start_time + execution_delay + timelock_duration`).
   - Any external caller triggering `execute_multisig_proposal` after `now > expiry` will permanently revert the status to `ProposalExpired`.
3. **Checked Arithmetic:** Calculations defining `expiry` limits employ strictly safe checked bounds (`checked_add`) to protect against any numeric overflow attacks forcing arbitrary expirations.

## Invariants Maintained
- Only one execution successfully concludes with an `Ok()` return per unique `proposal_id`.
- The number of unique approvals (`approvals.len()`) must safely satisfy the static `MultisigConfig::threshold` at the execution block timestamp.
- Proposal timelines are canonically anchored to `env.ledger().timestamp()`.
