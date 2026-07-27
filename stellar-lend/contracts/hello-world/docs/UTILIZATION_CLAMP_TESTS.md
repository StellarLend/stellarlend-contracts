# Utilization clamp tests

## Rationale

The `calculate_utilization` helper converts protocol totals into a utilization value expressed in basis points. The regression tests in this package verify the non-panicking edge cases around zero deposits, full utilization, and exact ratios that are important for downstream interest-rate math.

## Worked example

If the protocol has 1,000 deposits and 250 borrows, the utilization is:

$$
\frac{250 \times 10{,}000}{1{,}000} = 2{,}500 \text{ bps}
$$

That corresponds to 25% utilization. The tests assert that this value is returned exactly for representative ratios such as 25%, 50%, and 80%.

## Edge cases covered

- Zero deposits return zero instead of triggering a divide-by-zero panic.
- Borrows that meet or exceed deposits clamp to the maximum utilization of 10,000 bps.
- All returned values remain within the closed interval $[0, 10{,}000]$.
