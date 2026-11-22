# Multisig Execution Router

## Overview

The execution router turns `execute_proposal` from a status flip into a real,
typed dispatch.  A `ProposalAction` enum is stored on every `Proposal` and
dispatched to the matching on-chain handler when the proposal passes quorum and
is executed.

## Action Taxonomy

| Variant | Fields | Effect |
|---------|--------|--------|
| `SetThreshold` | `u32` | Updates the minimum approval count for future proposals |
| `RotateSigners` | `Vec<Address>` | Replaces the entire signer set |
| `InvokeContract` | `Address, Symbol, Vec<Val>` | Cross-contract call to a lending upgrade entrypoint |
| `SetExecutionDelay` | `u32` | Updates the minimum execution delay (in ledgers) between quorum and execution |

## Security Properties

1. **Payload-hash binding** – The `payload_hash` committed at creation is
   checked again at execution, preventing the action from being swapped between
   approval and execution.
2. **Quorum guard** – Execution is rejected unless the proposal is in `Passed`
   status (i.e. it has collected at least `threshold` distinct signer approvals).
3. **Execution Timelock Guard** – Proposals cannot be executed before `passed_at + execution_delay` ledgers have elapsed. If attempted earlier, execution reverts with `TimelockNotElapsed`. Proposals modifying execution delay are self-governed and cannot bypass the active delay.
4. **Emergency Cancellation Window** – Signers may cancel a `Passed` proposal during the delay window before execution becomes unlocked. Once the timelock elapses, cancellation is rejected.
5. **Expiry guard** – A proposal past its `expires_at` ledger is automatically
   marked `Expired` and cannot be executed.
6. **Idempotency guard** – A proposal in `Executed` or `Cancelled` state panics
   on re-execution attempts.

## Worked Dispatch Example

```
1. Signer A calls create_proposal(
       action = InvokeContract(InvokeContractParams {
           contract  = <lending-upgrade-addr>,
           fn_symbol = Symbol::new(&env, "upgrade_execute"),
           args_hash = sha256(abi_encode(new_wasm_hash)),
       }),
       payload_hash = sha256(abi_encode(action)),
       ttl_ledgers  = 1000,
   )  →  proposal_id = 0

2. Signer A calls approve_proposal(id = 0)   // 1 of 2 required
3. Signer B calls approve_proposal(id = 0)   // 2 of 2 → status Passed

4. Signer A calls execute_proposal(
       id           = 0,
       payload_hash = sha256(abi_encode(action)),   // must match step 1
   )
   // Router dispatches env.invoke_contract(lending-upgrade-addr, "upgrade_execute", [])
   // Emits ProposalExecutedEvent { id: 0, action_kind: "InvokeContract", ok: true }
```
