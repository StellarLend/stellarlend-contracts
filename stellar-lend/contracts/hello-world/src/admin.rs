//! # Admin and Access Control Module
//!
//! Provides a production-grade role-based access control (RBAC) and two-step
//! administrative transfer mechanism for the protocol.
//!
//! ## Features
//! - **Two-Step Admin Transfer**: Prevents accidental loss of super-admin authority
//!   via a `transfer_admin` → `accept_admin` workflow.
//! - **Role Registry**: Modular role-based authorization (e.g., `oracle_admin`, `risk_admin`).
//! - **Hardened Security**: Explicit `require_auth()` enforcement and storage-efficient
//!   state management.
//! - **Event Auditing**: Detailed event emission for all role and admin lifecycle changes.

use soroban_sdk::{contracterror, contracttype, Address, Env, IntoVal, Symbol, Val, Vec};
use crate::prelude::*;

/// Errors that can occur during admin operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AdminError {
    /// Unauthorized access - caller is not admin or lacks required role
    Unauthorized = 1,
    /// Invalid parameter value
    InvalidParameter = 2,
    /// Admin has already been set
    AdminAlreadySet = 3,
    /// No pending admin transfer exists
    NoPendingAdmin = 4,
}

/// Storage keys for Admin and Roles
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum AdminDataKey {
    /// The canonical super admin address
    Admin,
    /// The pending admin address awaiting acceptance
    PendingAdmin,
    /// Specific role assigned to an address: Role(RoleName, Address) -> bool
    Role(Symbol, Address),
    /// List of all defined role names in the protocol
    RoleRegistry,
}

// ============================================================================
// Super Admin Management
// ============================================================================

/// Check if the super admin is set
pub fn has_admin(env: &Env) -> bool {
    env.storage().persistent().has(&AdminDataKey::Admin)
}

/// Get the current actual super admin address
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&AdminDataKey::Admin)
}

<<<<<<< feature/hello-world-admin-roles
/// Get the current pending admin address awaiting acceptance
pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&AdminDataKey::PendingAdmin)
}

/// Initialize the super admin. Can only be called once when no admin exists.
/// Used during contract bootstrap.
pub fn set_admin(env: &Env, new_admin: Address) -> Result<(), AdminError> {
    if has_admin(env) {
        return Err(AdminError::AdminAlreadySet);
=======
/// Initialize super admin. Can only be called once or by existing admin.
///
/// # Authorization
///
/// - If no admin exists: Any caller can initialize (bootstrap mode)
/// - If admin exists: Only existing admin can modify (must pass caller parameter)
/// - Uses address comparison for verification, not require_auth() (bootstrap scenario)
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `new_admin` - The new admin address
/// * `caller` - The caller address (must be the current admin if one exists)
pub fn set_admin(env: &Env, new_admin: Address, caller: Option<Address>) -> Result<(), AdminError> {
    if let Some(current_admin) = get_admin(env) {
        if let Some(ref c) = caller {
            if *c != current_admin {
                return Err(AdminError::Unauthorized);
            }
        } else {
            return Err(AdminError::Unauthorized);
        }
>>>>>>> main
    }

    env.storage()
        .persistent()
        .set(&AdminDataKey::Admin, &new_admin);

    // Emit event
    let topics = (Symbol::new(env, "admin_changed"),);
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "new_admin").into_val(env));
    data.push_back(new_admin.into_val(env));

    env.events().publish(topics, data);
    Ok(())
}

/// Initiates a two-step transfer of super-admin rights.
///
/// The current admin must authorize this call. The `new_admin` will not
/// have authority until they call `accept_admin`.
pub fn transfer_admin(env: &Env, claimant: &Address, new_admin: Address) -> Result<(), AdminError> {
    require_admin(env, claimant)?;

    env.storage()
        .persistent()
        .set(&AdminDataKey::PendingAdmin, &new_admin);

    // Emit event
    let topics = (Symbol::new(env, "admin_transfer_started"), claimant.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "proposed_admin").into_val(env));
    data.push_back(new_admin.into_val(env));

    env.events().publish(topics, data);
    Ok(())
}

/// Accepts the pending super-admin transfer.
///
/// The proposed `new_admin` must authorize this call. Replaces the current
/// admin and clears the pending state.
pub fn accept_admin(env: &Env, claimant: &Address) -> Result<(), AdminError> {
    let pending_admin = get_pending_admin(env).ok_or(AdminError::NoPendingAdmin)?;
    if pending_admin != *claimant {
        return Err(AdminError::Unauthorized);
    }
    pending_admin.require_auth();

    env.storage()
        .persistent()
        .set(&AdminDataKey::Admin, &pending_admin);
    env.storage()
        .persistent()
        .remove(&AdminDataKey::PendingAdmin);

    // Emit event
    let topics = (Symbol::new(env, "admin_transfer_accepted"),);
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "new_admin").into_val(env));
    data.push_back(pending_admin.into_val(env));

    env.events().publish(topics, data);
    Ok(())
}

<<<<<<< feature/hello-world-admin-roles
/// Require that the claimant is the current super admin.
///
/// Uses both explicit address check and Soroban `require_auth()`.
/// This ensures security in production and correctness in mock tests.
pub fn require_admin(env: &Env, claimant: &Address) -> Result<(), AdminError> {
=======
/// Require that caller is super admin
///
/// # Authorization
///
/// Uses address comparison against stored admin address.
/// This is a custom authorization pattern for super admin verification.
/// Does not use require_auth() - caller must be verified before calling.
///
/// # Security
///
/// This function should be called after the caller has been authenticated
/// via require_auth() or in contexts where authentication is already verified.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), AdminError> {
>>>>>>> main
    let admin = get_admin(env).ok_or(AdminError::Unauthorized)?;
    if admin != *claimant {
        return Err(AdminError::Unauthorized);
    }
    admin.require_auth();
    Ok(())
}

<<<<<<< feature/hello-world-admin-roles
// ============================================================================
// Role Registry & Management
// ============================================================================

/// Grant a specific role to an address.
///
/// Only the super admin is authorized to manage roles.
pub fn grant_role(env: &Env, claimant: &Address, role: Symbol, account: Address) -> Result<(), AdminError> {
    require_admin(env, claimant)?;
=======
/// Grant a specific role to an address (admin only)
///
/// # Authorization
///
/// Uses `require_admin()` which verifies the caller is the super admin.
/// The caller must also authenticate via `require_auth()`.
/// This ensures only the super admin can delegate roles.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address
/// * `role` - The role to grant
/// * `account` - The address to grant the role to
pub fn grant_role(
    env: &Env,
    caller: Address,
    role: Symbol,
    account: Address,
) -> Result<(), AdminError> {
    require_admin(env, &caller)?;
>>>>>>> main

    let key = AdminDataKey::Role(role.clone(), account.clone());
    env.storage().persistent().set(&key, &true);

    // Update Role Registry
    let mut registry: Vec<Symbol> = env
        .storage()
        .persistent()
        .get(&AdminDataKey::RoleRegistry)
        .unwrap_or_else(|| Vec::new(env));
    
    let mut exists = false;
    for r in registry.iter() {
        if r == role {
            exists = true;
            break;
        }
    }
    if !exists {
        registry.push_back(role.clone());
        env.storage()
            .persistent()
            .set(&AdminDataKey::RoleRegistry, &registry);
    }

    // Emit event
    let topics = (Symbol::new(env, "role_granted"), role, account);
    env.events().publish(topics, ());

    Ok(())
}

<<<<<<< feature/hello-world-admin-roles
/// Revoke a specific role from an address.
///
/// Only the super admin is authorized to manage roles.
pub fn revoke_role(env: &Env, claimant: &Address, role: Symbol, account: Address) -> Result<(), AdminError> {
    require_admin(env, claimant)?;
=======
/// Revoke a specific role from an address (admin only)
///
/// # Authorization
///
/// Uses `require_admin()` which verifies the caller is the super admin.
/// The caller must also authenticate via `require_auth()`.
/// This ensures only the super admin can remove roles.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address
/// * `role` - The role to revoke
/// * `account` - The address to revoke the role from
pub fn revoke_role(
    env: &Env,
    caller: Address,
    role: Symbol,
    account: Address,
) -> Result<(), AdminError> {
    require_admin(env, &caller)?;
>>>>>>> main

    let key = AdminDataKey::Role(role.clone(), account.clone());
    env.storage().persistent().remove(&key);

    // Emit event
    let topics = (Symbol::new(env, "role_revoked"), role, account);
    env.events().publish(topics, ());

    Ok(())
}

/// Check if an address holds a specific role.
pub fn has_role(env: &Env, role: Symbol, account: Address) -> bool {
    let key = AdminDataKey::Role(role, account);
    env.storage().persistent().get(&key).unwrap_or(false)
}

/// Returns a list of all roles currently defined in the protocol.
pub fn get_role_registry(env: &Env) -> Vec<Symbol> {
    env.storage()
        .persistent()
        .get(&AdminDataKey::RoleRegistry)
        .unwrap_or_else(|| Vec::new(env))
}

/// Require that the caller is either the super admin or has the required role.
pub fn require_role_or_admin(env: &Env, caller: Address, required_role: Symbol) -> Result<(), AdminError> {
    // Check for super admin first
    if let Some(admin) = get_admin(env) {
        if admin == caller {
            admin.require_auth();
            return Ok(());
        }
    }

    // Check for role
    if has_role(env, required_role, caller.clone()) {
        caller.require_auth();
        return Ok(());
    }

    Err(AdminError::Unauthorized)
}
