# Rate Hysteresis Band

## Definition
The lending rate model supports an optional hysteresis band, configured as `hysteresis_bps` on `RateParams`.

Let:
- `current` be the previously applied borrow rate.
- `target` be the instantaneous utilization-derived borrow rate.
- `band` be `hysteresis_bps`.

Behavior:
- If `|target - current| <= band`, the effective rate is held at `current`.
- If `target > current + band`, smoothing proceeds toward `target - band`.
- If `target < current - band`, smoothing proceeds toward `target + band`.

This preserves the existing convergence behavior for sustained moves while filtering out micro-utilization noise that would otherwise churn the rate every ledger.

A `hysteresis_bps` value of `0` exactly preserves the prior behavior.

## Worked Example
Assume:
- `current = 1,700 bps`
- `target = 2,700 bps`
- `hysteresis_bps = 100 bps`
- `max_rate_change_per_ledger_bps = 50 bps`
- `elapsed = 1 ledger`

1. The raw gap is `2,700 - 1,700 = 1,000 bps`, which is outside the band.
2. The band-adjusted target becomes `2,700 - 100 = 2,600 bps`.
3. The maximum one-ledger move is `50 bps`.
4. The new effective rate is `1,700 + min(2,600 - 1,700, 50) = 1,750 bps`.

If the target later jitters to `1,780 bps`, the gap from `1,700 bps` is only `80 bps`, which is inside the `100 bps` band, so the effective rate remains `1,700 bps`.

## Interaction With Floor/Ceiling Clamp
Hysteresis is applied before the existing smoothing step. The result is still passed through the normal floor/ceiling clamp in `compute_borrow_rate`/`update_and_get_rate` flow.

That means:
- the band suppresses small target changes,
- smoothing limits per-ledger movement for larger changes,
- and the final effective rate still cannot move below `rate_floor_bps` or above `rate_ceiling_bps`.

For example, if smoothing would move the rate to `1,775 bps` but `rate_ceiling_bps = 1,760`, the stored effective borrow rate remains clamped at `1,760 bps`.
