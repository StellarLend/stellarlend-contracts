# TWAP Oracle

The AMM exposes a Uniswap-v2-style time-weighted average price (TWAP) oracle.
Prices are accumulated on-chain in fixed point on **every reserve-mutating
entrypoint**, stored in a fixed-size ring buffer, and queried through
`consult(window_secs)`.

This replaces the previous, unused `record_twap_observation` /
`get_twap_observations` design (which never ran and stored an unbounded list).

---

## Public API

| Function | Returns | Description |
|---|---|---|
| `consult(env, window_secs)` | `Result<(i128, i128), AmmPoolError>` | TWAP over the last `window_secs`, as `(price_a_in_b, price_b_in_a)`, both scaled by `TWAP_PRICE_SCALE`. `window_secs == 0` returns the spot price. |
| `observation_cardinality(env)` | `u32` | Number of retained observations (`0..=48`). |
| `get_observation(env, i)` | `Result<TwapObservation, AmmPoolError>` | Observation in **physical** ring slot `i`. |

`price_a_in_b` is the number of token-B base units per token-A base unit
(i.e. `reserve_b / reserve_a`), multiplied by `10^18` and floor-divided.

```rust
const SCALE: i128 = 1_000_000_000_000_000_000; // TWAP_PRICE_SCALE
// 1 A = 2 B  ->  price_a_in_b == 2 * SCALE
```

`TwapObservation` is:

```rust
pub struct TwapObservation {
    pub timestamp: u64,
    pub price0_cumulative: i128, // cumulative sum of price_a_in_b * elapsed_secs
    pub price1_cumulative: i128, // cumulative sum of price_b_in_a * elapsed_secs
}
```

---

## Accumulation

Both cumulative values are in units of `price * seconds`, so their magnitude is
the scaled price times the age of the pool. For a pool that trades near parity
(`price ≈ 10^18`) the accumulator gains `10^18` per second, i.e. about
`3.15 * 10^25` per year. `i128` holds roughly `1.7 * 10^38`, leaving a horizon
of ~5.4 * 10^12 years before wrapping. Wrapping is still used so the operation
can never panic (see [Overflow policy](#overflow-policy)).

An interval `[last_ts, now]` contributes `price * (now - last_ts)` using the
reserves **as they were before the current operation**:

- `now == last_ts` (multiple operations in one ledger): the interval is empty.
  No observation is written and no timestamp is duplicated. Only the most
  recent observation is needed to price any window, so a same-ledger flash
  swap round trip cannot move the TWAP.
- either reserve `<= 0`: there is no defined price, so the accumulator is
  frozen for that interval and no observation is written. This is only
  reachable for an empty pool (operations on an empty pool revert earlier) or
  a hypothetical one-sided pool.
- otherwise: prices are computed with checked arithmetic
  (`reserve * 10^18 / reserve'`), added with wrapping arithmetic, and one
  observation is written per ledger timestamp.

The hook runs *immediately before* the reserve writes, so the trade currently
being executed is never folded into the interval it just closed. This applies
to:

| Entrypoint | Reserves used for `[last_ts, now]` |
|---|---|
| `init_pool` | reserves already in storage (normally zero) |
| `add_liquidity` | pre-deposit reserves |
| `remove_liquidity` | pre-withdrawal reserves |
| `swap_a_for_b` / `swap_b_for_a` | pre-swap reserves |
| `flash_swap_a_for_b` | pre-flash reserves (stored in `KEY_FLASH_PRE_RESERVES`) |
| `repay_flash_swap` | pre-flash reserves from `KEY_FLASH_PRE_RESERVES` (never the in-flight, imbalanced reserves) |

Because `repay_flash_swap` may occur in a later ledger, the pre-flash reserves
are persisted at flash time and removed at repayment. An in-flight flash swap
therefore never leaks an arbitrageable spot price into the TWAP.

---

## Ring buffer

Storage:

| Key | Value |
|---|---|
| `TwapKey::State` (`("pool", "twap_state")`) | `TwapState { price0_cumulative, price1_cumulative, last_ts, index, cardinality }` |
| `TwapKey::Obs(i)` (`("pool", "twap_obs", i)`) | `TwapObservation`, for `i` in `0..48` |
| `KEY_FLASH_PRE_RESERVES` (`("pool", "flash_pre_reserves")`) | `(reserve_a, reserve_b)` while a flash swap is open |

`TWAP_OBSERVATION_COUNT = 48`. Observations are written to physical slots
`0, 1, 2, …`, wrapping back to `0` once the ring is full. The newest
observation is at `state.index`; when the ring is full the oldest is at
`(state.index + 1) % 48`, so chronological iteration is:

```text
for k in 0..cardinality:
    slot = (ring_start + k) % 48
```

Oracle storage is bounded: after 48 observations every write overwrites an
existing slot, so the number of ledger entries does not grow with swap
activity. `get_observation(i)` accepts physical slots only; off-chain
consumers that need a price should call `consult`.

---

## `consult`

Let `now` be the ledger timestamp and `target = now - window_secs`.

1. `window_secs == 0` → the current spot price from the stored reserves.
2. Every retained observation with `timestamp <= target` is a valid window
   start; the **newest** such observation is selected (binary search over the
   chronological ring order). If none exists, history is insufficient.
3. The newest cumulative is extrapolated to `now` using the current reserves:
   `cumulative_now = state.cumulative + current_price * (now - state.last_ts)`.
4. The result is
   `(cumulative_now - selected.cumulative) / (now - selected.timestamp)`.

The selected start is at least `window_secs` old, so the effective window is
**never shorter** than requested (it can be up to one observation interval
longer). With 48 slots and typical operation frequency the windowing error is
small; if the pool has fewer than 48 observations, the oldest one bounds the
available history.

Errors:

| Error | Condition |
|---|---|
| `EmptyPool` (1) | Either stored reserve is zero. |
| `TwapInsufficientHistory` (22) | `window_secs > now`, or no retained observation is old enough. |
| `TwapObservationNotFound` (23) | `get_observation` was given an out-of-range or unwritten slot (also an internal corruption guard). |
| `Overflow` (4) | A spot price `reserve * 10^18` does not fit in `i128` (reserves above ~`1.7 * 10^20`). |

---

## Manipulation resistance

Spot reserves can be moved arbitrarily within one ledger, but an interval of
length zero contributes nothing and only one observation is written per
timestamp. Therefore a swap-and-reverse (or any sequence of operations) inside
a single ledger cannot change the value returned by `consult(w)` for `w > 0`.
Moving the TWAP requires holding a distorted price across at least one full
ledger timestamp, and the distortion is weighted by how long it is held. This
is the standard Uniswap-v2 argument and is covered by
`single_block_manipulation_does_not_move_twap` and
`flash_in_flight_imbalance_never_enters_oracle`.

---

## Overflow policy

- Cumulative updates use `wrapping_add` / `wrapping_mul`: they can never panic,
  and with the ~5 * 10^12 year horizon described above the wrapping point is
  not reachable in practice.
- `consult` differences between two cumulatives use `wrapping_sub` for the
  same reason.
- Spot-price computation is **checked**: `reserve * 10^18` must fit in `i128`,
  otherwise `Overflow` is returned (never a panic). This bounds usable reserves
  to about `1.7 * 10^20` base units.
- All timestamp subtraction uses `saturating_sub` / `checked_sub`, so a
  backwards test clock or a window longer than the ledger timestamp yields a
  typed error instead of an underflow.

---

## Cost

`update_oracle` performs one state read, one observation write (only when the
timestamp advanced), and one state write. `consult` performs `O(log 48)`
storage reads. Because the ring is fixed-size, neither cost grows with pool
activity; `swap_cost_is_independent_of_ring_occupancy` compares a saturated
ring with a nearly empty one under the same invocation count.

---

## Tests

`src/twap_test.rs` covers exact constant-price behavior, segmented weighted
averages, same-timestamp operations, ring wrap/retention, insufficient
history, every reserve-mutating entrypoint (including a cross-ledger flash
swap), single-block manipulation, backwards timestamps, cost, and a proptest
against an independent `f64` reference model.
