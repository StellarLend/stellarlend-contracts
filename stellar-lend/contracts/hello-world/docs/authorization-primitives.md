# Authorization Primitives for Admin Operations

## Overview

This document describes the authorization primitives and authentication patterns used throughout the StellarLend protocol for privileged administrative operations. The codebase primarily uses Soroban's built-in `require_auth()` mechanism combined with role-based access control.

## Authentication Mechanisms

### 1. Soroban `require_auth()`

The primary authentication primitive used is Soroban's built-in `require_auth()` method:

```rust
// Basic authentication - caller must sign the transaction
caller.require_auth();

// Example from governance.rs
pub fn add_guardian(env: &Env, caller: Address, guardian: Address) -> Result<(), GovernanceError> {
    caller.require_auth();  // Ensures caller signed the transaction
    // ... rest of function
}
```

**Security Properties:**
- **Cryptographic Verification**: Uses the underlying Stellar network's signature verification
- **Non-repudiation**: Caller cannot deny having authorized the operation
- **Atomic**: Authentication is verified before any state changes occur
- **Protocol Agnostic**: Works with both Ed25519 and secp256k1 keys supported by Stellar

### 2. Role-Based Access Control (RBAC)

The protocol implements a custom RBAC system in `admin.rs`:

```rust
// Super admin verification
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), AdminError> {
    let admin = get_admin(env).ok_or(AdminError::Unauthorized)?;
    if admin != *caller {
        return Err(AdminError::Unauthorized);
    }
    Ok(())
}

// Role-based verification
pub fn require_role_or_admin(
    env: &Env,
    caller: &Address,
    required_role: Symbol,
) -> Result<(), AdminError> {
    if get_admin(env).map(|a| a == *caller).unwrap_or(false) {
        return Ok(());  // Super admin bypass
    }

    if has_role(env, required_role, caller.clone()) {
        return Ok(());  // Has required role
    }

    Err(AdminError::Unauthorized)
}
```

**Role Types:**
- **Super Admin**: Ultimate authority over the entire protocol
- **Role-based**: Optional specific roles (e.g., "oracle_admin", "pause_admin")

### 3. Multisig Authorization

Critical operations require multisig approval:

```rust
// Multisig admin verification
pub fn ms_approve(env: &Env, approver: Address, proposal_id: u64) -> Result<(), GovernanceError> {
    approve_proposal(env, approver, proposal_id)  // Includes auth check
}

// Internal auth check in governance.rs
let multisig_config: MultisigConfig = env
    .storage()
    .instance()
    .get(&GovernanceDataKey::MultisigConfig)
    .ok_or(GovernanceError::NotInitialized)?;

if !multisig_config.admins.contains(&approver) {
    return Err(GovernanceError::Unauthorized);
}
```

**Multisig Properties:**
- **Threshold-based**: Requires N-of-M approvals
- **Proposal System**: Changes must be proposed and approved
- **Time-delay**: Optional timelock for critical operations
- **Audit Trail**: All approvals are recorded and emit events

## Authorization Patterns by Module

### Governance Module (`governance.rs`)

| Function | Auth Pattern | Description |
|-----------|-------------|-------------|
| `initialize()` | `caller.require_auth()` | Admin initialization |
| `add_guardian()` | Admin check + `require_auth()` | Only super admin can add guardians |
| `remove_guardian()` | Admin check + `require_auth()` | Only super admin can remove guardians |
| `set_guardian_threshold()` | Admin check + `require_auth()` | Only super admin can set threshold |
| `start_recovery()` | Guardian check + `require_auth()` | Only guardians can initiate recovery |
| `approve_recovery()` | Guardian check + `require_auth()` | Only guardians can approve recovery |
| `execute_recovery()` | `caller.require_auth()` | Anyone can execute approved recovery |

### Admin Module (`admin.rs`)

| Function | Auth Pattern | Description |
|-----------|-------------|-------------|
| `set_admin()` | Optional admin check | Bootstrap or admin transfer |
| `require_admin()` | Address comparison | Super admin verification |
| `grant_role()` | `require_admin()` | Only admin can grant roles |
| `revoke_role()` | `require_admin()` | Only admin can revoke roles |
| `require_role_or_admin()` | Role or admin check | Flexible authorization |

### Multisig Module (`multisig.rs`)

| Function | Auth Pattern | Description |
|-----------|-------------|-------------|
| `ms_set_admins()` | Bootstrap or admin check | Initialize or update admin set |
| `ms_propose_set_min_cr()` | Admin check + `require_auth()` | Only admins can propose |
| `ms_approve()` | Admin check + `require_auth()` | Only admins can approve |
| `ms_execute()` | Admin check + `require_auth()` | Only admins can execute |
| `set_ms_threshold()` | Admin check + `require_auth()` | Only admins can set threshold |

### Recovery Module (`recovery.rs`)

| Function | Auth Pattern | Description |
|-----------|-------------|-------------|
| `set_guardians()` | Multisig admin check | Only multisig admins can set guardians |
| `add_guardian()` | Multisig admin check | Only multisig admins can add |
| `remove_guardian()` | Multisig admin check | Only multisig admins can remove |
| `set_guardian_threshold()` | Multisig admin check | Only multisig admins can set threshold |
| `start_recovery()` | Guardian check + `require_auth()` | Only guardians can initiate |
| `approve_recovery()` | Guardian check + `require_auth()` | Only guardians can approve |
| `execute_recovery()` | `caller.require_auth()` | Anyone can execute approved |

## Key Security Assumptions

### Cryptographic Assumptions
1. **Stellar Network Security**: Assumes Stellar's signature verification is secure
2. **Key Management**: Assumes private keys are properly secured by administrators
3. **Network Consensus**: Assumes transaction finality and ordering guarantees

### Protocol Assumptions
1. **Admin Trust Model**: Super admin has ultimate authority and is trusted
2. **Multisig Distribution**: Multisig admins are independent and non-colluding
3. **Guardian Distribution**: Social recovery guardians are independent and trustworthy
4. **Role Separation**: Different roles are held by different entities

### Operational Assumptions
1. **Timely Response**: Guardians and multisig admins respond in reasonable time
2. **Key Backup**: Administrators maintain secure backups of keys
3. **Access Control**: Physical and operational access to admin keys is controlled

## Threat Mitigation

### 1. Unauthorized Access
- **Mitigation**: `require_auth()` ensures cryptographic proof of identity
- **Coverage**: All privileged operations require authentication

### 2. Key Compromise
- **Mitigation**: Multisig requires multiple compromised keys
- **Recovery**: Social recovery mechanism for admin key rotation

### 3. Rogue Admin
- **Mitigation**: Guardian-based recovery can remove compromised admins
- **Checks**: Critical operations may require multiple approvals

### 4. Replay Attacks
- **Mitigation**: Stellar network prevents transaction replay
- **Additional**: Nonce and sequence numbers in transactions

## Best Practices

### For Administrators
1. **Use Hardware Wallets**: Store admin keys in secure hardware
2. **Multisig Distribution**: Distribute multisig keys across different entities
3. **Regular Rotation**: Periodically rotate admin and guardian keys
4. **Access Logging**: Monitor all administrative operations

### For Developers
1. **Consistent Auth**: Always use `require_auth()` for privileged operations
2. **Principle of Least Privilege**: Grant minimum necessary permissions
3. **Event Emission**: Emit events for all administrative actions
4. **Input Validation**: Validate all parameters after authentication

### For Protocol Operations
1. **Threshold Planning**: Set appropriate multisig and guardian thresholds
2. **Recovery Testing**: Regularly test social recovery mechanisms
3. **Key Backup**: Maintain secure, distributed key backups
4. **Incident Response**: Have procedures for key compromise scenarios

## Cryptographic Primitive Support

The StellarLend protocol supports both cryptographic primitives used by Stellar:

### Ed25519
- **Default**: Most common key type on Stellar
- **Performance**: Fast signature verification
- **Compatibility**: Widely supported by wallets and tools

### secp256k1
- **Compatibility**: Ethereum and Bitcoin ecosystem compatibility
- **Hardware Support**: Broad hardware wallet support
- **Interoperability**: Easier integration with existing infrastructure

Both key types provide equivalent security guarantees for the protocol's authorization needs. The choice between them should be based on operational considerations rather than security differences.

## Conclusion

The StellarLend protocol uses a defense-in-depth approach to authorization:
1. **Cryptographic Authentication** via Soroban's `require_auth()`
2. **Role-Based Access Control** for operational flexibility
3. **Multisig Protection** for critical operations
4. **Social Recovery** for key compromise scenarios

This combination provides strong security guarantees while maintaining operational flexibility and recoverability.
