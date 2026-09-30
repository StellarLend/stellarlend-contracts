# Protocol Insurance Fund

The StellarLend Protocol Insurance Fund serves as a first-loss capital buffer to absorb bad debt before any losses are socialized or passed to depositors.

---

## 1. Funding Formula

The insurance fund is dynamically funded by a configured percentage share of all accrued borrow interest, as well as optional explicit admin deposits.

### Dynamic Funding (Interest Share)
During any interest-accruing user operation (such as borrowing, repaying, or liquidating), the interest accrued is calculated. A percentage of this interest is directed to the insurance fund:

$$\text{Insurance Share} = \frac{\text{Accrued Interest} \times \text{Insurance Share BPS}}{10,000}$$

#### Configuration Bounds
* **Minimum Share:** $0\text{ BPS}$ ($0\%$)
* **Maximum Share:** $10,000\text{ BPS}$ ($100\%$)
* Configured via `set_insurance_share(env, share_bps)`.

---

## 2. Draw-Down Order

When a borrower position is liquidated and their debt exceeds their available collateral, a shortfall arises. The shortfall is processed in the following order:

1. **First-Loss Buffer (Insurance Fund):** The protocol attempts to cover the shortfall using the accumulated balance of the `InsuranceFund`.
   $$\text{Insurance Drawn} = \min(\text{Shortfall}, \text{Insurance Fund Balance})$$
2. **Residual Socialization (Bad Debt Ledger):** Any remaining shortfall that cannot be covered by the insurance fund is added to the bad debt ledger to be eventually written off or socialized across depositors.
   $$\text{Residual Shortfall} = \text{Shortfall} - \text{Insurance Drawn}$$

### Safety Guarantees
* **No Negative Balance:** Since the drawn amount is capped at $\min(\dots, \text{Insurance Fund Balance})$, the insurance fund balance can never be reduced below zero.
* **No Over-payment:** The drawn amount is capped at the shortfall, meaning the fund will never pay more than the exact debt shortfall.

---

## 3. Worked Example

### Scenario A: Full Coverage
* **Insurance Fund Balance:** $100\text{ tokens}$
* **Liquidation Shortfall:** $40\text{ tokens}$

1. **Draw Calculation:**
   $$\text{Drawn} = \min(40, 100) = 40\text{ tokens}$$
2. **Update Balances:**
   * **Insurance Fund:** $100 - 40 = 60\text{ tokens}$
   * **Bad Debt Ledger Increase:** $40 - 40 = 0\text{ tokens}$

*Result: The shortfall is fully absorbed by the insurance fund. Depositors bear zero loss.*

### Scenario B: Partial Coverage (Residual Socialization)
* **Insurance Fund Balance:** $30\text{ tokens}$
* **Liquidation Shortfall:** $75\text{ tokens}$

1. **Draw Calculation:**
   $$\text{Drawn} = \min(75, 30) = 30\text{ tokens}$$
2. **Update Balances:**
   * **Insurance Fund:** $30 - 30 = 0\text{ tokens}$
   * **Bad Debt Ledger Increase:** $75 - 30 = 45\text{ tokens}$

*Result: The insurance fund is depleted to 0, and the remaining 45 tokens are recorded on-ledger as bad debt.*
