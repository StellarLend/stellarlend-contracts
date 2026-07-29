# Validator-Set Churn Limit

This document describes the design, rationale, and implementation details of the maximum single-rotation validator-set churn limit.

## Rationale

By default, the bridge contract's `rotate_validators` function allows replacing the entire validator set in a single epoch transition. While correct under normal operation, this presents a significant security risk: if a quorum of the current validator set is compromised, the attackers can immediately rotate the validator set to an attacker-controlled set in a single step.

Once the attacker-controlled set is in place, the attackers gain permanent control over the bridge, allowing them to sign arbitrary messages, bypass the rolling value caps over time, and permanently hijack the bridge.

To mitigate this threat, we introduce a configurable **maximum single-rotation validator-set churn limit** (`max_churn`). This limit restricts the number of validator additions and removals (the symmetric difference between the old and new sets) that can occur in a single rotation. As a result:
- A complete takeover of the validator set requires multiple, sequential rotations.
- Each intermediate rotation must be signed by the preceding set.
- This slows down any potential takeover attempt, making it observable on-chain and giving the bridge operators/guardians time to intervene (e.g., triggering a pause or emergency shutdown).

## Churn Computation

The churn is defined as the size of the symmetric difference between the current validator set $A$ and the new validator set $B$. The sets are deduplicated to ensure that duplicate keys do not artificially inflate or deflate the count.

$$\text{Churn} = |(A \setminus B) \cup (B \setminus A)| = |A \setminus B| + |B \setminus A|$$

- **Added Validators ($B \setminus A$):** Validators present in the new set but not in the current set.
- **Removed Validators ($A \setminus B$):** Validators present in the current set but not in the new set.

### Worked Example

Suppose the current validator set is:
$$A = \{V_1, V_2, V_3, V_4\}$$

The new validator set is proposed as:
$$B = \{V_1, V_2, V_5, V_6\}$$

1. **Added:** $\{V_5, V_6\}$ (count = 2)
2. **Removed:** $\{V_3, V_4\}$ (count = 2)
3. **Total Churn:** $2 + 2 = 4$

If `max_churn` is configured to `2`, this rotation will be **rejected** because the churn of $4 > 2$.
If `max_churn` is configured to `4` or is `None` (disabled), this rotation will be **accepted**.

## Edge Cases

- **Unset Limit (`None`):** When `max_churn` is `None`, no limit is enforced, preserving the original behavior (arbitrary churn is allowed).
- **Empty/Singleton Sets:** Standard minimum/maximum validator set sizes and quorum-proof verification are still fully enforced. The churn limit is checked in addition to, not instead of, these rules.
- **Duplicate Keys:** Both sets are fully deduplicated before computing the symmetric difference.
- **Checked Arithmetic:** The addition of `added` and `removed` counts is performed using checked arithmetic (`checked_add`) to prevent any potential panic/overflow.
