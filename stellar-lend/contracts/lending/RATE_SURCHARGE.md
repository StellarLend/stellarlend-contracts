# Emergency Rate Surcharge

The borrow-rate model now supports an optional emergency surcharge for extreme utilization.
This is intended to increase the cost of borrowing when liquidity is tight and to further incentivize repayment or additional deposits during a rush.

## Rationale

The original curve already steepens above the main kink, but it can still flatten under the ceiling as utilization approaches 100%. The emergency surcharge adds an additional band above a configurable threshold so the curve keeps pushing upward in a liquidity crunch.

The surcharge is only applied when utilization exceeds the configured surcharge kink, and it is layered on top of the existing rate curve before the standard floor and ceiling clamps are applied.

## Worked Example

Assume the following parameters:

- base rate: 100 bps
- main kink: 8,000 bps
- multiplier: 2,000 bps
- jump multiplier: 10,000 bps
- surcharge kink: 8,000 bps
- surcharge slope: 5,000 bps

At 90% utilization, the base curve gives:

- pre-kink rate: $100 + \frac{8,000 \times 2,000}{10,000} = 1,700$
- jump portion: $\frac{(9,000 - 8,000) \times 10,000}{10,000} = 1,000$
- total before surcharge: $2,700$
- surcharge: $\frac{(9,000 - 8,000) \times 5,000}{10,000} = 500$
- final rate: $3,200$

This keeps the upward pressure intact even as the standard curve approaches the ceiling.

## Edge Cases

- The surcharge is disabled by default when both new fields are left at their zero values.
- The surcharge does not change the sub-kink path or the existing main-kink logic.
- The final rate remains bounded by the configured floor and ceiling.
- Monotonicity is preserved: higher utilization never produces a lower borrow rate.
