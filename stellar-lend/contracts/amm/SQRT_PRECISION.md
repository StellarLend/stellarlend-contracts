# Square Root Precision Guarantee

The `sqrt` function provided in `amm::math::sqrt` computes the exact integer floor of the real square root for all non-negative `i128` values. 

Formally, for any non-negative integer `y`, the returned integer `r = sqrt(y)` satisfies the precision bound:

`r^2 <= y < (r + 1)^2`

This mathematical property is guaranteed across the full valid input range (0 to `i128::MAX`). 

## Edge Cases and Boundaries
- The implementation natively handles inputs of `0` and `1`.
- Negative inputs will result in a panic as they do not have a real number representation.
- The boundary near `i128::MAX`, including the largest representable perfect square (`13043817825332782212^2`), is fully supported. Note that for values very close to `i128::MAX`, evaluating `(r + 1)^2` might overflow standard `i128` bounds and requires evaluation in a wider type like `u128` to verify the mathematical upper bound.
