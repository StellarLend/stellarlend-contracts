# Soroban storage TTL policy

Soroban storage TTL is measured in ledgers, not seconds. At the usual five
seconds per ledger, `DAY_IN_LEDGERS` is 17,280. Network ledger timing may vary,
so this conversion is an operational estimate rather than a wall-clock
guarantee.

The shared policy in `stellar-lend-common::ttl` refreshes instance and
persistent storage when fewer than seven days remain, extending the lifetime
to 30 days. Both the threshold and extension amount are capped to the
environment's `max_ttl()`; the resulting extension amount never exceeds that
maximum. Missing persistent keys are not created or extended.

State-changing entrypoints refresh instance storage. Persistent entries are
refreshed when written and when they are used by state-changing operations or
keepers. Pure views do not pay the cost of extending TTL. A permissionless
keeper entrypoint refreshes each contract's global state, while unbounded
per-user or per-proposal entries are refreshed when touched and may require a
targeted keeper call after long inactivity.

This policy does not migrate or restore already-archived entries. Existing
storage keys remain in their current persistent tier to preserve compatibility
with deployed instances. Long-lived grants and replay markers need periodic
activity/keeper refresh before their maximum TTL elapses; this policy does not
make persistent entries permanent.
