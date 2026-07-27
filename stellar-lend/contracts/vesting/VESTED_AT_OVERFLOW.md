# Checked-Arithmetic & Overflow Prevention in `vested_at`

## Rationale

In linear vesting schedules, the amount of tokens vested at any given pause-adjusted timestamp is calculated as:
$$\text{vested} = \frac{\text{elapsed} \times \text{total\_amount}}{\text{duration\_secs}}$$

When using large token principal amounts (e.g. up to `i128::MAX`) and long durations or timestamps, calculating the intermediate product $\text{elapsed} \times \text{total\_amount}$ directly can easily exceed the capacity of `u128` (and certainly `u64`), causing an arithmetic overflow/panic even though the final result is mathematically bounded by the principal.

To prevent intermediate overflow, we reorder and partition the calculation using **split quotient-remainder multiplication/division** (mul-div style).

---

## The Solution

Since we only calculate the linear vesting fraction when $\text{elapsed} < \text{duration\_secs}$, we can divide the principal ($\text{total\_amount}$) into quotient and remainder relative to the denominator ($\text{duration\_secs}$):

1. Let $P = \text{total\_amount}$
2. Let $D = \text{duration\_secs}$
3. Let $E = \text{elapsed}$

We divide $P$ by $D$:
$$P = q \cdot D + r \quad \text{where} \quad q = \lfloor P / D \rfloor, \quad r = P \bmod D$$

Substituting $P$ into the linear formula:
$$\text{vested} = \frac{E \cdot (q \cdot D + r)}{D} = E \cdot q + \frac{E \cdot r}{D}$$

Since $E < D$:
- $E \cdot q < D \cdot q \le P$, so the term $E \cdot q$ is strictly bounded by the principal and cannot overflow `u128`.
- $r < D \le \text{u64::MAX}$ and $E < D \le \text{u64::MAX}$. The product $E \cdot r$ is the product of two numbers less than $\text{u64::MAX}$, which fits comfortably inside a `u128` (maximum possible product is $\approx 2^{128}$ which fits in `u128::MAX`).
- Therefore, the second term $\lfloor (E \cdot r) / D \rfloor$ is also fully safe from overflow and is strictly less than $E < D$.
- The sum $(E \cdot q) + \lfloor (E \cdot r) / D \rfloor$ is mathematically less than $P \le \text{i128::MAX}$, meaning the final sum will never overflow `i128`.

---

## Worked Example

Suppose:
- $\text{total\_amount} (P) = 2^{120} \approx 1.329 \times 10^{36}$ (fits in `i128`)
- $\text{duration\_secs} (D) = 10^9$ seconds ($\approx 31.7$ years)
- $\text{elapsed} (E) = 5 \times 10^8$ seconds (exactly half-way through the vesting schedule)

### Old Direct Multiplication Math:
$$\text{intermediate product} = E \times P = (5 \times 10^8) \times 2^{120} \approx 6.64 \times 10^{44}$$
Since $2^{128} \approx 3.402 \times 10^{38}$, the intermediate product $E \times P$ overflows `u128` and causes a panic.

### New Quotient-Remainder Partitioned Math:
1. $q = P / D = 2^{120} / 10^9 \approx 1.329 \times 10^{27}$
2. $r = P \bmod D = 2^{120} \bmod 10^9 = 483,648,000$
3. $\text{val1} = E \times q = (5 \times 10^8) \times (2^{120} / 10^9) \approx 6.645 \times 10^{35}$
4. $\text{val2} = (E \times r) / D = (5 \times 10^8 \times 483,648,000) / 10^9 = 241,824,000$
5. $\text{vested} = \text{val1} + \text{val2} \approx 6.645 \times 10^{35}$

Neither of the intermediate steps overflows `u128`. The exact vested amount is computed safely without panic.

---

## Edge Case Notes

1. **Max Principal & Max Duration:** If $P = \text{i128::MAX}$ and $D = \text{u64::MAX}$, and $E = \text{u64::MAX} - 1$, the math handles the boundaries securely.
2. **Elapsed Past Duration:** If $\text{elapsed} \ge \text{duration\_secs}$, the function bypasses the linear calculation and directly returns the full principal, protecting the division step and capping the return value at the principal.
3. **Zero Principal / Zero Duration:** Safe guards in the initialization of grants (`total_amount <= 0 || duration_secs == 0`) prevent division by zero. A fallback check `total_amount <= 0` returns 0 immediately.
