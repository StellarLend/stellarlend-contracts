# Governance Participation Quorum

## Overview

The participation quorum is a minimum-turnout requirement that must be satisfied
before a proposal can be queued for execution. It is independent of — and checked
in addition to — the approval threshold.

Without a quorum guard a proposal could pass with a single yes-vote out of
thousands of eligible voters. The quorum ensures that at least a meaningful
fraction of the electorate weighs in before a high-impact parameter change takes
effect.

## Configuration

`quorum_bps` is stored in `GovernanceConfig` and set at `gov_initialize`:

| Field | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `quorum_bps` | `u32` | `5000` | `0–10000` | Minimum participation in basis points |

`5000` bps = 50 %, `2500` bps = 25 %, `0` = quorum disabled.

## Formula

```
participation_bps = (yes_votes + no_votes) × 10 000 / total_voters
```

The check inside `queue_proposal`:

```rust
if participation_bps < config.quorum_bps as i128 {
    return Err(GovernanceError::QuorumNotMet);
}
```

`total_voters` is `config.voters.len()`.  When the voter list is empty the check
is skipped entirely (division-by-zero guard) and the proposal proceeds normally.

## Worked Example

**Setup:** 4 eligible voters, `quorum_bps = 5000` (50 %).

| Scenario | yes | no | participation_bps | Passes? |
|---|---|---|---|---|
| Nobody votes | 0 | 0 | 0 | ❌ QuorumNotMet |
| One voter (25 %) | 1 | 0 | 2500 | ❌ QuorumNotMet |
| Two voters (50 %) | 2 | 0 | 5000 | ✅ Approved |
| Two voters, mixed | 1 | 1 | 5000 | ✅ Approved |
| Three voters (75 %) | 3 | 0 | 7500 | ✅ Approved |

## Relationship to Approval Threshold

The quorum check and the approval threshold are independent gates:

```
queue_proposal succeeds iff:
    participation_bps >= quorum_bps          (quorum gate)
    AND yes_votes / (yes_votes + no_votes) >= approval_threshold  (threshold gate)
```

The current implementation enforces the quorum gate; the approval threshold is a
separate concern that can be layered on top.

## Error

`GovernanceError::QuorumNotMet` (code `10`) is returned by `gov_queue_proposal`
when the participation constraint is not satisfied.  Callers should surface this
to the user so they can solicit additional votes before re-attempting to queue.
