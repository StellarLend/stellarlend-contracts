# Protocol Configuration Module (`config.rs`)

The `config` module provides a flexible, key-value storage system for the StellarLend protocol's configuration parameters. It allows the protocol admin to efficiently add, retrieve, back up, and restore configuration settings without needing to upgrade the entire contract.

## Capabilities

- **`config_set`**: Allows the admin to set a specific configuration key to a `Val` payload.
- **`config_get`**: Allows anyone to retrieve the `Val` associated with a specific configuration key.
- **`config_backup`**: Allows the admin to retrieve multiple configuration key-value pairs simultaneously for backup processes.
- **`config_restore`**: Allows the admin to efficiently restore a vector of configuration key-value pairs, typically after an upgrade or migration.

## Security Considerations

- **Admin-Only Writes**: The functions `config_set`, `config_backup`, and `config_restore` enforce stringent access controls by utilizing `require_admin`. Any access attempts by non-admin addresses will be halted and result in an `Unauthorized` error.
- **Arbitrary Data (`Val`) Storage**: Since `Val` can be any Soroban SDK primitive, care must be taken by the admin to ensure the correct types are passed to avoid unintended decoding errors elsewhere in the contract.
- **Data Persistence**: Configuration parameters are stored directly in `persistent` storage using the `ConfigDataKey::ConfigKey(Symbol)` key to ensure they easily survive contract upgrades and remain unarchived.

## NatSpec Documentation

All public APIs within `config.rs` and their exposed counterparts in `lib.rs` are thoroughly documented utilizing Rust's idiomatic documentation comments (`///`), achieving NatSpec-style inline documentation that correctly describes method arguments and expected behaviors.

### Example Usage

```rust
// Setting a configuration value (Admin Only)
let key = Symbol::new(&env, "fee_rate");
let val = 100_u32.into_val(&env);
client.config_set(&admin, &key, &val);

// Getting a configuration value (Public)
let retrieved_val = client.config_get(&key).unwrap();
```
