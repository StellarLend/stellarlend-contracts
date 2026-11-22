# Isolation ceilings and reserve invariants

This document describes the accounting contract for isolated collateral. It is
intended to be read with `src/isolation_invariants_test.rs` and the isolation
helpers in `src/lib.rs`.

## Purpose

An isolated collateral asset has a governance-defined debt ceiling. The ceiling
limits the total outstanding debt attributed to that collateral asset. The
limit is a reserve-wide capacity constraint, rather than a per-user limit.

The contract stores the capacity state in two related values:

| Value | Meaning |
| --- | --- |
| `AssetIsolation(asset).isolation_debt_ceiling` | Maximum tracked debt for the asset |
| `IsolationDebt(asset)` | Current tracked outstanding debt for the asset |

The central invariant is:

```text
0 <= IsolationDebt(asset) <= isolation_debt_ceiling(asset)
```

The right-hand side applies while isolation is enabled. A disabled asset keeps
its historical tracker for auditability, but new borrowing no longer consumes
isolation capacity and the disabled asset has no active ceiling.

## State-transition rules

### Enabling or updating isolation

An administrator may configure a non-negative ceiling. A positive ceiling is
required when isolation is enabled. An update is accepted only when the new
ceiling is at least the already tracked debt. This prevents governance from
creating a configuration that is invalid at the moment it is written.

The checks happen before the configuration is persisted. A rejected update
therefore leaves both the existing isolation flag and the existing ceiling
unchanged.

The following transitions are valid:

| Current mode | New mode | New ceiling | Result |
| --- | --- | --- | --- |
| disabled | disabled | `0` | Clear active configuration |
| disabled | enabled | `debt + capacity` | Start tracking new capacity |
| enabled | enabled | `debt` | Freeze borrowing capacity |
| enabled | enabled | `debt + capacity` | Increase available capacity |
| enabled | disabled | `0` | Remove active cap, preserve tracker |

Negative ceilings are never valid. A zero ceiling is meaningful only when
disabling isolation; accepting it for an enabled asset would create an
ambiguous configuration and make every future borrow fail for the wrong
reason.

### Borrowing

The isolation check is applied to the actual debt delta that will be recorded,
not merely to the caller's requested amount. This matters because borrowing
also passes through health and cross-asset calculations that can reduce the
effective amount.

The transition is ordered as follows:

1. Authenticate the caller and validate the requested amount.
2. Accrue and calculate the position's effective debt delta.
3. Check the effective delta against the current isolation bucket.
4. Update the user position, total debt, and isolation tracker.
5. Emit the normal borrow event.

No tracker write occurs before the ceiling check. If the check rejects the
borrow, the user's position and the reserve bucket remain unchanged.

For an isolated asset, the checked update is:

```text
new_isolation_debt = old_isolation_debt + effective_delta
new_isolation_debt <= ceiling
```

The addition is checked as well as ceiling-bounded. A numeric overflow is an
invariant error, not a reason to wrap the bucket to a smaller value.

### Repayment and liquidation release

Repayment releases only the amount actually applied to outstanding debt. It
does not release the caller's requested amount when that request is larger than
the position. The resulting update is:

```text
new_isolation_debt = old_isolation_debt - actual_repaid
```

The subtraction is checked. A tracker that is smaller than the amount being
released indicates corrupted or incompatible state and returns
`IsolationDebtInvariant`. Saturating to zero would hide the corruption and
allow subsequent borrowers to consume capacity that is still represented by
debt.

The contract runtime rolls back the complete invocation on this error. This
means the user position and tracker cannot diverge if release validation fails.
The same accounting rule applies to liquidation paths that release debt from an
isolated position: release is performed once, after the final debt delta is
known, and the checked decrement is part of the same invocation.

## Disabled isolation

Disabling isolation is an explicit policy change. It does not rewrite historical
tracked debt, because that value is useful for migrations, audits, and safely
re-enabling the asset later. While disabled:

- new borrows do not increment the isolation tracker;
- new borrows are not rejected by the old ceiling;
- repayments of debt that was tracked before disabling still release exactly
  the applied amount;
- re-enabling must choose a ceiling that covers the retained tracker.

Keeping these behaviors separate avoids silently losing reserve accounting while
also allowing governance to remove a cap during an emergency or migration.

## Asset isolation boundaries

Every tracker is keyed by the collateral asset address. A borrow against one
isolated asset cannot consume capacity belonging to another isolated asset.
Tests cover two independent assets at their exact ceilings, as well as an
unconfigured non-isolated asset, to protect this namespace boundary.

Ledger time advancement alone does not change the tracker. Interest accrual or
another explicit state transition may change debt according to the contract's
normal accounting rules, but an idle read or a ledger sequence change cannot
create or release isolation capacity.

## Failure and rollback expectations

The following failures must be atomic:

| Failure | State that must remain unchanged |
| --- | --- |
| Borrow exceeds ceiling | User debt, total debt, tracker |
| Ceiling is below current debt | Isolation flag, ceiling, tracker |
| Tracker increment overflows | User position and tracker |
| Tracker decrement underflows | User position and tracker |
| Invalid ceiling value | Isolation configuration |

These expectations are tested through the generated client `try_` methods. The
tests compare the full debt position and the bucket before and after rejected
calls, rather than checking only the returned error.

## Test matrix

`isolation_invariants_test.rs` exercises the following scenarios:

1. Update a ceiling below outstanding debt.
2. Freeze capacity at exactly current debt.
3. Disable isolation while retaining historical debt.
4. Reject negative ceilings and invalid enabled zero ceilings.
5. Repeated partial repayment and borrow cycles.
6. Full repayment followed by reuse of the exact capacity.
7. Rejected borrow with unchanged position and tracker.
8. Independent buckets for independent isolated assets.
9. Non-isolated borrowing with no tracker creation.
10. Public validation of non-positive ceiling-check amounts.
11. Failed release with rollback of position and tracker.
12. Ledger advancement without an implicit capacity change.
13. Ceiling increase limited to the newly added capacity.
14. Failed governance update preserving the old configuration.

Together, the tests cover boundaries, sequential reuse, administrative
updates, namespace isolation, and failure atomicity. They are intentionally
small deterministic scenarios so a future property-test suite can reuse the
same state-transition model and vary the operation sequence.

## Review checklist

When changing isolation logic, reviewers should confirm:

- the check uses the effective debt delta;
- tracker updates use checked arithmetic;
- every increment has one matching release path;
- partial repayment releases only actual repayment;
- governance cannot lower a ceiling below current debt;
- disabled mode is explicit and does not erase historical accounting;
- rejected calls leave position and tracker state unchanged;
- tests cover exact ceiling, exact repayment, and cross-asset boundaries;
- event and total-debt changes remain in the same transaction as tracker changes.

This checklist is part of the implementation contract for issue #1899.
