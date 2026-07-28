# Normalize Price Tests

## Rationale
The `normalize_price` and `normalize_price_ceil` functions are critical for converting asset prices with varying decimal scales to a common internal scale (18 decimals). This ensures accurate value aggregation across assets with different price decimal precisions.

- `normalize_price`: Uses floor division when scaling down, conservative for collateral valuation.
- `normalize_price_ceil`: Uses ceiling division when scaling down, conservative for debt valuation.

## Worked Numeric Example: Floor vs Ceil Scaling
Let's take an example where:
- Raw price = 123_456_789
- Asset decimals = 20
- Internal decimals = 18

To normalize, we divide by 10^(20-18) = 100.

### Floor Calculation
`123_456_789 / 100 = 1_234_567` (integer division truncates)

### Ceil Calculation
`(123_456_789 + 100 - 1) / 100 = 123_456_888 / 100 = 1_234_568`

## Edge Case Notes
1. **Zero raw price**: Always maps to zero for both functions.
2. **Exact multiples**: When raw price is an exact multiple of the scaling factor, floor and ceil give the same result.
3. **Overflow**: Both functions return `None` instead of panicking when arithmetic overflow occurs.
4. **Same decimals**: No conversion needed; functions return the raw price as-is.
