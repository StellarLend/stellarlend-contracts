# Security Notes & Trust Boundaries

## Trust Boundaries
- **Admins:** The highest level of privilege. Admins can update parameters (such as minimum borrow amounts, deposit ceilings, and oracles), pause the protocol, trigger emergency shutdown, and designate guardians. They are also responsible for upgrading the protocol.
- **Guardians:** Designed for rapid response. Guardians can only trigger emergency shutdowns. They cannot upgrade contracts, unpause the system, or change parameters.
- **Users:** End-users interact with the protocol via `deposit`, `borrow`, `repay`, and `withdraw` mechanisms subject to protocol checks. User operations are sandboxed to their respective `Address` scopes.
- **Oracles:** Trusted entities providing price feeds used for health factor checks. If an oracle becomes malicious, it could trigger improper liquidations, but internal checks restrict maximum liquidation amounts (via close factor limits).

## Authorization Model
All external entry points modifying state or user balances call `user.require_auth()`. This delegates authorization entirely to the Soroban SDK's robust authorization framework. 
Protocol functions restricted to Admins enforce validation via `admin.require_auth()` and ensure the caller matches the registered Admin in the data store.

## Reentrancy Protections
In Soroban, contract logic guarantees atomicity. However, as an added measure against logic-based reentrancy across cross-contract calls:
- All external calls to update state (e.g. `save_deposit_position`) occur *before* external token transfers where applicable (the Checks-Effects-Interactions pattern).
- High-risk operations are guarded by global pause mappings which an Admin or Guardian can engage via the pause module if anomalous behavior occurs.

## Arithmetic Bounds
Protocol parameters strictly utilize `checked_add`, `checked_sub`, `checked_mul`, and `checked_div` to prevent overflow and underflow paths. Zero-amount and uninitialized parameter paths intentionally return structured `ContractError` values rather than panicking where possible.
