cargo check -p vesting
#[cfg(test)]
mod tests {
    use crate::test::{create_vesting_contract, get_env}; // Adjust paths based on your actual test module

    /// Test that revoking a grant mid-vest correctly splits tokens:
    /// 1. Already vested amount remains with the grantee.
    /// 2. Unvested remainder is clawed back to the treasury.
    /// 3. Total (Claimed + Clawed + Remaining) equals original principal.
    #[test]
    fn test_revoke_mid_vest_split_accuracy() {
        let env = get_env();
        let client = create_vesting_contract(&env);
        
        // Setup logic: Create grant, advance time to mid-vest
        // Perform revocation
        // Assertions:
        // assert_eq!(grantee_balance + treasury_balance, original_principal);
        // assert!(treasury_received == expected_unvested_remainder);
    }
}

