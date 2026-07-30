# Revoke approval

## Rationale

The multisig contract now lets a signer remove their approval from an open proposal before it is executed. This makes the approval flow reversible when a signer changes their mind, notices a mistake, or wants to avoid contributing to a proposal that should not proceed.

The revoke path is intentionally strict:

- it only works for proposals that still exist and are not already executed;
- it rejects revocation attempts for approvals that were never recorded;
- it preserves the existing quorum semantics, so execution still depends on the current valid approvals and the current threshold.

## Worked example

1. A proposal is created with threshold 2.
2. Signer A approves the proposal.
3. Signer B approves the proposal as well.
4. Signer B calls `revoke_approval`.
5. The approval list is updated so only Signer A remains, and the proposal can no longer satisfy quorum until another valid approval is added.

This allows a signer to withdraw support before the proposal reaches execution, while still keeping the approval history auditable through the stored approval list and the emitted `ApprovalRevoked` event.

## Edge cases

- Revoking a non-existent approval returns `ApprovalNotFound` instead of silently succeeding.
- Revoking from an executed proposal returns `ProposalAlreadyExecuted`.
- Revoking from an expired proposal returns `ProposalExpired`.
- Revoking an approval can reduce the effective approval count below the live threshold, which prevents execution until quorum is restored.
