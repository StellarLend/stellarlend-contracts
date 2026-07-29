# Borrow-Rate Smoothing and Convergence Proof

## Rationale
StellarLend's interest rate model avoids instantaneous spikes in borrow rates when utilization sharply changes (e.g., due to flash loans or large withdrawals). Instead of instantly adopting the new raw rate, it moves the current rate toward a computed target rate using an exponential moving average (EMA) approach based on a smoothing factor.

## The Recurrence Relation
Let $R_t$ be the current rate, $R_{target}$ be the target rate, and $\alpha$ be the smoothing factor scaled by the basis point denominator ($10,000$).
The naive continuous recurrence is:
$$R_{t+1} = R_t + (R_{target} - R_t) \cdot \frac{\alpha}{10,000}$$

This is a standard EMA and acts as a strict contraction mapping toward $R_{target}$ because $0 < \alpha < 10,000$. The distance $|R_{target} - R_{t+1}|$ shrinks by a factor of $(1 - \frac{\alpha}{10,000})$ each step.

## Saturation Bound and Clamp Interaction
In a purely mathematical (continuous) context, an EMA never strictly reaches the target; it only approaches it asymptotically. Furthermore, in integer arithmetic, division truncates toward zero. This means that if $|R_{target} - R_t| \cdot \alpha < 10,000$, integer division will yield a change of $0$, effectively stalling convergence prematurely (undershoot).

To prevent this **saturation bound**, the implementation introduces a forced minimum step of $1$ basis point in the direction of the target if the computed change evaluates to $0$ (and $R_t \neq R_{target}$). This guarantees that:
1. Convergence does not stall due to integer truncation.
2. The sequence converges to exactly $R_{target}$ in finite steps without overshoot.

**Floor/Ceiling Clamp Interaction:**
The `compute_borrow_rate` function clamps the raw rate between `rate_floor_bps` and `rate_ceiling_bps`. Because the smoothing recurrence only interpolates between $R_t$ and the clamped $R_{target}$ (and the saturation step is only $1$), it is impossible for the smoothed rate to violate the floor or ceiling constraints, provided the initial $R_0$ was valid.

## Worked Numeric Traces
We demonstrate convergence with $\alpha = 2,000$ ($20\%$).

### Upward Convergence
- **Initial Rate ($R_0$)**: $100$
- **Target Rate**: $110$
- **Delta**: $10$

| Step | Current Rate | Delta | Raw Change (Delta * 2000) | Div 10000 | Applied Change (Sat.) | Next Rate |
|------|--------------|-------|---------------------------|-----------|-----------------------|-----------|
| 1    | 100          | +10   | +20000                    | +2        | +2                    | 102       |
| 2    | 102          | +8    | +16000                    | +1        | +1                    | 103       |
| 3    | 103          | +7    | +14000                    | +1        | +1                    | 104       |
| 4    | 104          | +6    | +12000                    | +1        | +1                    | 105       |
| 5    | 105          | +5    | +10000                    | +1        | +1                    | 106       |
| 6    | 106          | +4    | +8000                     | 0         | +1 (Saturation)       | 107       |
| 7    | 107          | +3    | +6000                     | 0         | +1 (Saturation)       | 108       |
| 8    | 108          | +2    | +4000                     | 0         | +1 (Saturation)       | 109       |
| 9    | 109          | +1    | +2000                     | 0         | +1 (Saturation)       | 110       |
| 10   | 110          | 0     | 0                         | 0         | 0                     | 110       |

### Downward Convergence
- **Initial Rate ($R_0$)**: $210$
- **Target Rate**: $200$
- **Delta**: $-10$

| Step | Current Rate | Delta | Raw Change (Delta * 2000) | Div 10000 | Applied Change (Sat.) | Next Rate |
|------|--------------|-------|---------------------------|-----------|-----------------------|-----------|
| 1    | 210          | -10   | -20000                    | -2        | -2                    | 208       |
| 2    | 208          | -8    | -16000                    | -1        | -1                    | 207       |
| 3    | 207          | -7    | -14000                    | -1        | -1                    | 206       |
| 4    | 206          | -6    | -12000                    | -1        | -1                    | 205       |
| 5    | 205          | -5    | -10000                    | -1        | -1                    | 204       |
| 6    | 204          | -4    | -8000                     | 0         | -1 (Saturation)       | 203       |
| 7    | 203          | -3    | -6000                     | 0         | -1 (Saturation)       | 202       |
| 8    | 202          | -2    | -4000                     | 0         | -1 (Saturation)       | 201       |
| 9    | 201          | -1    | -2000                     | 0         | -1 (Saturation)       | 200       |
| 10   | 200          | 0     | 0                         | 0         | 0                     | 200       |

These exact numbers are verified programmatically in the `rate_smoothing_proof_doctest.rs` file.
