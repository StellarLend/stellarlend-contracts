# Soroban Timelock Module

## Overview

The `soroban-timelock` crate implements a secure, delayed execution pattern for privileged administrative actions in the StellarLend protocol. It acts as a safety buffer between the proposal of an operational change and its actual implementation.

## Security Context

Trust Boundaries:
- **Admin**: Authorized queueing and cancelling of actions. Bound by `min_delay` rules. Must actively monitor the mempool or protocol logs for scheduled actions. 
- **Grace Period**: Bound on standard execution availability preventing forgotten actions from becoming permanently valid "execution traps" in the distant future.

## Integration

1. A multi-sig or single governance admin deploys and initializes `soroban-timelock` with `min_delay` (e.g. 7 days) and `grace_period` (e.g. 14 days).
2. For an action, admin evaluates `env.ledger().timestamp()` and queues an action against a target address + function with `eta` satisfying the minimum delay constraint.
3. The community can introspect off-chain events published under `(timelock, queue)` to trace the payload hash (`BytesN<32>`) and verify the pending function signature.
4. After `eta` has passed, any authorized executor can trigger `execute`. 

## Flow

1. **Queue**: `queue(target_addr, SomeFunc_sym, args, ETA)` generates the action ID -> storage.
2. **Execute**: Validates the time bounds against `ETA`. Removes the action from persistent storage, then dynamically invokes the target contract.
3. **Cancel**: Optional reversion of the pending rule, useful if the community rejects the pending action or an error in `args` is discovered.
