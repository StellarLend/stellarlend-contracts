# Admin and Access Control

The StellarLend protocol operates securely by enforcing access controls for privileged functions such as changing risk configurations, updating interest rate models, configuring the oracle, and managing emergency pause states. The core element of this is the unified `admin` module.

## Super Admin

The protocol expects a single Super Admin to be created during contract initialization via the `lib::initialize(env, admin)` invocation. This stores an `Admin` record internally in persistent storage via `set_admin(&env, new_admin, None)`.

The Super Admin has default clearance to all privileged operations around the protocol. Their address can be verified natively by ensuring the caller equals the stored admin via `admin::require_admin(&env, &caller)`.
The Super Admin can seamlessly transfer their rights by calling `admin::set_admin(&env, new_admin, Some(current_admin))`.

## Roles

A flexible Role-Based Access Control (RBAC) was implemented to allow delegating specific operations to customized role types. Custom roles are identified by symbols (e.g. `Symbol::new(&env, "oracle_admin")`).

The Super Admin can define and map addresses to responsibilities:

- `admin::grant_role(&env, admin_address, role_symbol, address_to_grant)`
- `admin::revoke_role(&env, admin_address, role_symbol, address_to_revoke)`

To validate during executions you can authorize a particular action for either exactly a custom role or the super admin implicitly using `admin::require_role_or_admin(&env, &caller, role_symbol)`.

## Example Integrations

```rust
// In your module you want to safeguard
use crate::admin::{require_admin, require_role_or_admin};
use soroban_sdk::{Address, Env, Symbol};

pub fn sensitive_admin_operation(env: &Env, caller: Address) {
    require_admin(env, &caller).unwrap(); // Halts if not Super Admin
    // Code block executing protected changes
}

pub fn sensitive_role_operation(env: &Env, caller: Address) {
    let required_role = Symbol::new(&env, "oracle_admin");
    require_role_or_admin(env, &caller, required_role).unwrap(); // Halts if neither Super Admin nor has 'oracle_admin' mapping
    // Code block executing protected changes
}
```

## Events

The module naturally publishes notifications for external services to subscribe and ingest standard access alterations on-chain.

- `admin_changed`: Triggered when the super admin changes. Contains `new_admin` and `caller`.
- `role_granted`: Identifies a granted privilege containing `account` and topic-based `role`.
- `role_revoked`: Tracks when an account’s role is cleared. Contains `account` and topic-based `role`.
