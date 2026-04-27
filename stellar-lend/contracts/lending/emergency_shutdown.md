# Emergency Shutdown and Recovery Flow

This document describes the contracts-only emergency lifecycle implemented in the lending contract.

## State Machine

`Normal -> Shutdown -> Recovery -> Normal`

- `Normal`: regular operation.
- `Shutdown`: hard stop for high-risk operations.
- `Recovery`: controlled unwind mode where users can reduce risk.

## Roles

- `admin`: governance-controlled address. Can configure guardian and manage recovery lifecycle.
- `guardian`: optional fast-response address set by admin. Can trigger `emergency_shutdown`.

## Authorized Calls

- `set_guardian(admin, guardian)` -> admin only.
- `emergency_shutdown(caller)` -> admin or guardian.
- `start_recovery(admin)` -> admin only, only valid from `Shutdown`.
- `complete_recovery(admin)` -> admin only.

## Operation Policy by State

- `Normal`:
  - All operations follow existing granular pause rules.
- `Shutdown`:
  - Block: `deposit`, `deposit_collateral`, `borrow`, `liquidate`, `flash_loan`, `repay`, `withdraw`.
  - Allow: view/read methods and admin recovery actions.
- `Recovery`:
  - Allow: `repay`, `withdraw` (subject to granular pause and collateral checks).
  - Block: `deposit`, `deposit_collateral`, `borrow`, `liquidate`, `flash_loan`.

## Security Notes

- Emergency checks are enforced in both contract entrypoints and core borrow logic, including token-receiver deposit/repay paths.
- Recovery mode does not allow users to create new protocol exposure.
- Granular pauses still apply during recovery (for partial shutdown handling).
- All key transitions emit contract events (`guardian_set_event`, `emergency_state_event`, existing pause events).

## Test Coverage Added

`src/emergency_shutdown_test.rs` covers:

- unauthorized shutdown attempts,
- guardian and admin authorized transitions,
- shutdown blocking of high-risk operations,
- controlled recovery allowing unwind only,
- transition edge cases,
- partial shutdown controls during recovery.
