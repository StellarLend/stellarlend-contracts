# Emergency Rate Surcharge

The borrow-rate model supports an optional emergency surcharge for extreme utilization.
It increases the cost of borrowing when liquidity is tight and further incentivizes
repayment or additional deposits during a rush.

## Rationale

The standard kinked curve steepens above the main kink, but the rate can still flatten
as it approaches the configured ceiling. The emergency surcharge adds an extra linear
band above a configurable utilization threshold so the curve keeps pushing upward during
a liquidity crunch.

The surcharge is applied only when utilization **exceeds** the surcharge kink. It is
layered on top of the existing kinked curve, and the combined rate is still clamped by
the configured floor and ceiling.

## Worked Example

Assume the following parameters:

- base rate: 100 bps
- main kink: 8,000 bps (80% utilization)
- multiplier: 2,000 bps
- jump multiplier: 10,000 bps
- surcharge kink: 8,000 bps
- surcharge slope: 5,000 bps
- rate ceiling: 10,000 bps

At 90% utilization (9,000 bps):

1. Pre-kink portion: `100 + (8,000 × 2,000) / 10,000 = 1,700`
2. Jump portion: `(9,000 − 8,000) × 10,000 / 10,000 = 1,000`
3. Base curve total: `1,700 + 1,000 = 2,700`
4. Surcharge: `(9,000 − 8,000) × 5,000 / 10,000 = 500`
5. Final rate (before ceiling): `2,700 + 500 = 3,200`

Both surcharge fields default to zero, which disables the surcharge and preserves the
existing curve behavior.

## Edge Cases

- **Disabled by default**: When `surcharge_kink_bps` and `surcharge_slope` are both zero,
  no surcharge is applied.
- **Strictly above the kink**: At utilization equal to the surcharge kink, the surcharge
  is zero; it begins only for utilization strictly greater than the kink.
- **Independent of main kink**: The surcharge does not alter the sub-kink path or the
  existing main-kink jump logic.
- **Ceiling still applies**: The surcharge is added before clamping; the final rate cannot
  exceed `rate_ceiling_bps`.
- **Monotonicity**: Higher utilization never produces a lower borrow rate.
