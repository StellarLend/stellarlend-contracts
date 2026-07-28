# Multisig Action Allow-List

## Rationale

The multisig contract executes proposal payloads after approval and a timelock. That is powerful, but it also means proposal decoding and execution paths should be constrained as tightly as possible.

This allow-list narrows the governance attack surface by requiring every proposal action kind to be explicitly registered before it can be created or executed. If a future action type is added to the contract but not registered, proposals for that action fail closed with `MultisigError::ActionNotAllowed`.

The contract now enforces the allow-list in two places:

- `create_proposal`: prevents new proposals for unregistered action kinds from entering the queue.
- `execute_proposal`: re-checks the stored proposal action kind at execution time, so a kind that was removed after queueing cannot still execute later.

## Default Behavior

Initialization seeds the allow-list with the only action kind the contract currently supports:

- `ActionKind::SetThreshold`

That preserves today's behavior for threshold-change proposals and avoids regressions for existing callers.

## Worked Example

1. The admin initializes the contract. `SetThreshold` is automatically allowed.
2. The admin creates proposal `P1` to change the threshold from `3` to `5`.
3. Before `P1` reaches its execution ledger, the admin removes `SetThreshold` from the allow-list.
4. When someone tries to execute `P1`, `execute_proposal` re-checks the allow-list and returns `ActionNotAllowed`.
5. If the admin later re-adds `SetThreshold`, new threshold proposals can be created again.

## Edge Cases

- Removing an action kind does not delete already-queued proposals. It only prevents them from executing while the kind remains disallowed.
- Re-adding a removed kind does not mutate queued proposals; it simply makes that kind eligible again for new creation and later execution checks.
- The allow-list setters are admin-authenticated, matching the rest of the contract's management surface.
- Uninitialized contracts still reject admin management calls through the existing `NotInitialized` path because the admin must already exist before the allow-list can be managed.
