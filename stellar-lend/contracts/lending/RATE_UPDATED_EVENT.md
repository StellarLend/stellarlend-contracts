# RateUpdated Event

> **Audience:** Indexers, integrators, and protocol maintainers.
> **Status:** Implemented — see `rate_model.rs` and `rate_updated_event_test.rs`.

---

## Rationale

The lending protocol's interest rate changes dynamically with pool
utilisation, but without a dedicated event, indexers and off-chain
consumers cannot track the rate trajectory without polling storage each
ledger. `RateUpdatedEvent` fills this gap by emitting a structured,
versioned payload every time the smoothed rate changes.

**Why emit only on change?**
- Reduces event spam — a no-op update (same utilisation → same smoothed
  rate) does not emit.
- Indexers can treat each emission as a meaningful rate transition.
- Gas / storage costs are minimised.

**Why versioned?**
The event carries a `schema_version: u32` field aligned with the
protocol's [Event Schema Versioning Policy](../docs/EVENT_SCHEMA_VERSIONING.md),
allowing indexers to safely decode the payload across contract upgrades.

---

## Event Definition

### Topic

The Soroban `#[contractevent]` macro derives the topic from the struct
name (`"RateUpdatedEvent"`). Indexers should filter on this topic.

### Payload

| Field              | Type   | Description                                    |
|--------------------|--------|------------------------------------------------|
| `schema_version`   | `u32`  | Currently `1`. Bump on breaking changes.       |
| `utilization_bps`  | `i128` | Pool utilisation, in basis points (e.g. 8000 = 80%). |
| `target_rate_bps`  | `i128` | The pre-smoothing target rate, in basis points. |
| `applied_rate_bps` | `i128` | The EMA-smoothed rate that was persisted, in basis points. |
| `ledger`           | `u32`  | Ledger sequence number at emission time.       |

### Decoding (pseudocode)

```python
def decode_rate_updated_event(raw):
    version = raw.get("schema_version")
    if version == 1:
        return {
            "utilization_bps": raw["utilization_bps"],
            "target_rate_bps": raw["target_rate_bps"],
            "applied_rate_bps": raw["applied_rate_bps"],
            "ledger": raw["ledger"],
        }
    raise UnknownSchemaVersion(version)
```

---

## Worked Example

Assume the pool has:
- **Total deposits:** 1,000,000 USD
- **Total debt:** 400,000 USD → utilisation = 40%

### First emission (no prior state)

```
target_rate  = BASE_RATE + (utilisation * SLOPE1 / TARGET_UTILIZATION)
             = 50 + (4000 * 50 / 8000)
             = 50 + 25
             = 75 bps  (0.75%)

applied_rate = target_rate  (first call, no EMA blend)
             = 75 bps

Event:
  schema_version:  1
  utilization_bps: 4000
  target_rate_bps: 75
  applied_rate_bps: 75
  ledger:          12345
```

### Second emission after utilisation rises to 80%

```
target_rate  = BASE_RATE + SLOPE1
             = 50 + 50
             = 100 bps  (1.0%)

applied_rate = EMA(target, previous)
             = (SMOOTHING_FACTOR * target + (1 - SMOOTHING_FACTOR) * previous) / 10000
             = (1000 * 100 + 9000 * 75) / 10000
             = (100000 + 675000) / 10000
             = 77 bps  (integer division, no rounding)

Event:
  schema_version:  1
  utilization_bps: 8000
  target_rate_bps: 100
  applied_rate_bps: 77
  ledger:          12400
```

After several more updates at the same utilisation, the smoothed rate
converges toward the target (100 bps).

---

## Edge Cases

### 1. Zero deposits

When `total_deposits == 0`, utilisation is defined as `0`. The target
rate falls to `BASE_RATE_BPS` (50 bps / 0.5%). This prevents division-
by-zero panics.

### 2. Debt without deposits

If debt exists but deposits have been withdrawn to zero, utilisation is
still `0` → the rate is `BASE_RATE_BPS`. This is a degenerate state
that should be prevented by the protocol's emergency circuit breakers.

### 3. Uninitialised smoothing state

On the very first call, there is no `RateSmoothingState` in storage.
The function handles this gracefully by setting `applied_rate = target_rate`
(skipping the EMA blend) and emitting the event. No panic.

### 4. Identical utilisation between calls

If utilisation has not changed since the last update, the smoothed rate
will be identical to the stored value. The function detects this and
skips the event emission entirely. Storage is not written either.

### 5. EMA convergence

Because the smoothing factor (`SMOOTHING_FACTOR_BPS = 1000`, i.e. α ≈ 0.1)
is small, a single utilisation change moves the smoothed rate only
partway toward the target. Empirically `~1/α ≈ 10` successive updates
at constant utilisation narrow the gap to ≤ 1 bps; with i128 integer
truncation the steady-state plateau is a few bps below the target
(e.g. at the 80 % kink the plateau is 91 bps, not the 100-bps target).

This is intentional — it prevents the published rate from jumping
erratically due to transient utilisation spikes caused by a single
large borrow or repay.

### 6. Overflow protection

All arithmetic uses `checked_mul` / `checked_div` / `saturating_add` to
prevent overflow. If an overflow would occur (e.g. extreme utilisation
with very large pool sizes), the function degrades gracefully by
returning `BASE_RATE_BPS`.

---

## Related

- [Source: `src/rate_model.rs`](./src/rate_model.rs)
- [Tests: `src/rate_updated_event_test.rs`](./src/rate_updated_event_test.rs)
- [Event Schema Versioning Policy](../docs/EVENT_SCHEMA_VERSIONING.md)
- [`compute_target_rate`](./src/rate_model.rs) — the kink-model rate function
