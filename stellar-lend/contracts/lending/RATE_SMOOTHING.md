# Bounded Per-Ledger Rate Smoothing

## Context & Motivation
The Stellar Lend contract computes the borrow rate dynamically based on instantaneous pool utilization. While simple and capital-efficient, this approach is vulnerable to momentary utilization spikes. For example:
- A flash loan or a single large borrow/repay within a single ledger can temporarily spike utilization to 100%.
- Without smoothing, this instantaneous spike reprices the pool's borrow rate to its maximum ceiling, inflating the interest accrued for all active borrowers in that block.
- Attackers can exploit this to manipulate interest yields or grief other borrowers.

To neutralize single-block manipulation, this feature introduces an **optional rate-smoothing window**. Instead of jumping instantly to the utilization-based target rate, the effective rate moves toward the target rate by a bounded step per ledger.

---

## Smoothing Formula
Let:
- $R_{last}$ be the effective borrow rate applied in the previous update.
- $R_{target}$ be the target borrow rate computed from current utilization (clamped by floor and ceiling).
- $\Delta_{max}$ be the configured maximum rate change per ledger (in basis points).
- $L_{elapsed}$ be the number of ledgers closed since the last update.

The maximum allowed rate change for the update is:
$$Change_{max} = \Delta_{max} \times L_{elapsed}$$

The new effective borrow rate $R_{new}$ is:
$$R_{new} = R_{last} + \text{clamp}(R_{target} - R_{last}, -Change_{max}, Change_{max})$$

### Disable-ability
If $\Delta_{max} = \text{i128::MAX}$, the maximum change is unbounded. The effective rate moves instantly to the target rate, maintaining the contract's legacy behavior (smoothing disabled).

---

## Worked Example

### Scenario Configuration
- **Base Rate ($R_{base}$)**: $100\text{ bps}$ ($1\%$)
- **Floor / Ceiling**: $50\text{ bps}$ ($0.5\%$) / $10,000\text{ bps}$ ($100\%$)
- **Max Change Per Ledger ($\Delta_{max}$)**: $50\text{ bps}$ ($0.5\%$ rate change limit per block)
- **Initial State**:
  - $L_{last} = 100$
  - $R_{last} = 1,700\text{ bps}$ (utilization is at the kink, $80\%$)

---

### Step 1: Spike in Utilization (Ledger 101)
A flash loan borrow spikes utilization to $90\%$, driving the target rate to $2,700\text{ bps}$.

- **Current Ledger**: $101$
- **Elapsed Ledgers ($L_{elapsed}$)**: $101 - 100 = 1$
- **Target Rate ($R_{target}$)**: $2,700\text{ bps}$
- **Max Allowed Change ($Change_{max}$)**: $50 \times 1 = 50\text{ bps}$
- **Difference ($R_{target} - R_{last}$)**: $2,700 - 1,700 = +1,000\text{ bps}$
- **Applied Rate ($R_{new}$)**:
  $$R_{new} = 1,700 + \min(1,000, 50) = 1,750\text{ bps}$$

*Result*: The rate only moves up to $1,750\text{ bps}$ instead of jumping to the spiked $2,700\text{ bps}$, neutralising the attack.

---

### Step 2: Spike Reverted (Ledger 102)
The flash loan is repaid within the same block or next block, dropping utilization back to $80\%$, where the target rate is $1,700\text{ bps}$.

- **Current Ledger**: $102$
- **Elapsed Ledgers ($L_{elapsed}$)**: $102 - 101 = 1$
- **Target Rate ($R_{target}$)**: $1,700\text{ bps}$
- **Max Allowed Change ($Change_{max}$)**: $50 \times 1 = 50\text{ bps}$
- **Difference ($R_{target} - R_{last}$)**: $1,700 - 1,750 = -50\text{ bps}$
- **Applied Rate ($R_{new}$)**:
  $$R_{new} = 1,750 - \min(50, 50) = 1,700\text{ bps}$$

*Result*: The pool quickly and smoothly returns to its baseline rate of $1,700\text{ bps}$.

---

### Step 3: Sustained High Utilization (Ledgers 102 to 112)
Utilization rises to $90\%$ and stays there for $10$ blocks.

- **Initial State at Ledger 102**: $R_{last} = 1,700\text{ bps}$
- **Current Ledger**: $112$
- **Elapsed Ledgers ($L_{elapsed}$)**: $112 - 102 = 10$
- **Target Rate ($R_{target}$)**: $2,700\text{ bps}$
- **Max Allowed Change ($Change_{max}$)**: $50 \times 10 = 500\text{ bps}$
- **Difference ($R_{target} - R_{last}$)**: $2,700 - 1,700 = +1,000\text{ bps}$
- **Applied Rate ($R_{new}$)**:
  $$R_{new} = 1,700 + \min(1,000, 500) = 2,200\text{ bps}$$

---

### Step 4: Full Convergence (Ledger 122)
Sustained utilization continues for another $10$ blocks.

- **Initial State at Ledger 112**: $R_{last} = 2,200\text{ bps}$
- **Current Ledger**: $122$
- **Elapsed Ledgers ($L_{elapsed}$)**: $122 - 112 = 10$
- **Target Rate ($R_{target}$)**: $2,700\text{ bps}$
- **Max Allowed Change ($Change_{max}$)**: $50 \times 10 = 500\text{ bps}$
- **Difference ($R_{target} - R_{last}$)**: $2,700 - 2,200 = +500\text{ bps}$
- **Applied Rate ($R_{new}$)**:
  $$R_{new} = 2,200 + \min(500, 500) = 2,700\text{ bps}$$

*Result*: The effective rate converges fully to the target of $2,700\text{ bps}$ without overshooting.
