**Oracle Freshness & Fallback — Design and Validation Boundary**

Refs #1917

Purpose
- Define explicit invariants, validation boundary, failure semantics, and tests for oracle price reads and fallbacks. Keep changes limited to `stellar-lend/contracts/oracle` and `stellar-lend/contracts/lending/src`.

Scope & Goals
- Enforce freshness, decimal/scale normalization, authorized sources, ownership/authorization checks, and fail-closed fallback behavior.
- Validate route parameters, wallet identity, network, numeric ranges, timestamp/freshness, and server responses at the boundary between client/API and contract logic.
- Cover adversarial scenarios: replay, tamper, wrong-network, disconnected-wallet, malformed response.

State & Invariants
- Price tuple: (value: i128 or u128, scale: u8, source_id: SourceId, timestamp: u64, signature: Option<Signature>, nonce: Option<u64>)
- Freshness window: prices older than `FRESHNESS_WINDOW_SECONDS` MUST be rejected by default (fail-closed). Default: 120 seconds (configurable).
- Decimal invariants: price value must be normalized to a known scale; operations should convert using explicit rounding/truncation rules.
- Authorization: only known, configured oracle sources are trusted; any signed payload must verify against configured public keys.
- Fallback ordering: fallback sources may be used only when the primary source is validated and either absent or explicitly stale; fallback uses the same validations.
- Failure semantics: invalid/malformed/unsigned data => reject; do not silently accept weaker data.

Input Boundary Validation (what to check)
- Route / RPC params: network id, contract address, asset ids; validate type, length, and allowed values.
- Wallet identity: require on-chain ownership/authorization checks (owner pubkey / signer) rather than trusting local client state.
- Network ID: reject requests where the network param doesn't match the executing environment.
- Numeric values: ensure integers in expected range; reject NaN/inf; validate scale and enforce max/min.
- Timestamp/freshness: validate `timestamp <= now + ALLOWABLE_SKEW` and `now - timestamp <= FRESHNESS_WINDOW_SECONDS`.
- Response schema: use strict schema parsing; unknown/missing fields make the payload invalid.
- Signatures & nonces: verify signature against allowed public keys; check nonce to mitigate replay when available.

Replay & Tampering Protections
- Require signed payloads for off-chain oracle data where possible; verify signatures before accepting.
- For unauthenticated sources, treat responses as advisory only and require multiple consistent sources before acceptance.
- Include nonces or monotonic timestamps in signed payloads; reject duplicate nonces for the same source.

Fallback Policy
- Define explicit ordered list of sources per asset: primary -> fallback1 -> fallback2.
- Only attempt fallback when primary is provably stale/failed by validation.
- Each fallback entry must pass the same validations (freshness, signature/schema).
- Log and emit events when fallback occurs.

Retry / Backoff Behavior
- For transient network errors, perform up to `N_RETRIES` with exponential backoff (configurable) before considering source failed.
- Distinguish transient errors (timeouts, network) from permanent (malformed signature) and act accordingly.

Authorization & Ownership Checks
- Require explicit on-chain checks for payer/owner-sensitive flows in lending module. Do not infer ownership from wallet UI.
- Validate the signer(s) of transactions: confirmation that the signer is allowed to perform the action.

Failure Modes & Response
- On validation failure: return a clear error code and reason; do not return a numeric price.
- On partial failure (primary invalid, fallback valid): return the validated fallback and emit an event describing fallback reason.
- On full failure (no valid price): fail the higher-level operation (loan, liquidation read) with safe defaults (reject or pause action).

Testing Plan (automated)
- Unit tests
  - Schema validation: malformed/missing fields rejected.
  - Timestamp/freshness: stale timestamps rejected; boundary tests at FRESHNESS_WINDOW_SECONDS +/- 1.
  - Decimal normalization: scale conversion tests and rounding/truncation behavior.
  - Signature verification: valid and invalid signatures, missing signature.
  - Fallback logic: primary stale -> fallback accepted; primary valid -> fallback ignored.

- Integration tests
  - Simulated multiple oracle providers (mock servers) that return valid, stale, malformed, and replayed payloads.
  - Wrong-network: simulate request with mismatched network param and assert rejection.
  - Disconnected-wallet: simulate missing signer and assert authorization failure.
  - Replay attack: same signed payload replayed -> rejected by nonce/timestamp checks.

Validation Commands
- Run unit tests (JS/TS): `cd api && npm test` (if applicable)
- Run Rust tests (contracts): from `stellar-lend/contracts` run: `cargo test -p <contract-crate>` or `cargo test --all` (adjust per crate layout).

Design Tradeoffs
- Strict fail-closed posture increases safety but may reduce availability; mitigate with well-tested fallbacks and conservative freshness windows.
- Requiring signatures everywhere is safest but may be infeasible for some external providers; for those, require multi-source agreement.
- Nonces provide strong replay protection but require oracle support; where not available, rely on monotonic timestamps plus short windows.

Remaining Limitations
- Some external data providers may not provide signatures or nonces; in those cases we rely on multi-source agreement and shorter freshness windows.
- On-chain gas/perf constraints may require tuned limits (e.g., avoid heavy crypto checks in hot paths; validate off-chain and submit proofs on-chain when needed).

Next Steps
1. Review invariants and approve defaults for `FRESHNESS_WINDOW_SECONDS`, `ALLOWABLE_SKEW`, and retry counts.
2. Implement strict schema + signature + freshness checks in `stellar-lend/contracts/oracle`.
3. Add boundary checks in `stellar-lend/contracts/lending/src` where oracle reads are consumed; enforce authorization.
4. Add unit and integration tests and run CI.

Files to change (proposed)
- `stellar-lend/contracts/oracle/*` — schema parsing, signature verification, fallback logic, events.
- `stellar-lend/contracts/lending/src/*` — boundary checks for price reads and ownership checks before sensitive actions.

If you want, I will now implement the schema + freshness + signature checks in `stellar-lend/contracts/oracle` and add unit tests for the critical paths.
