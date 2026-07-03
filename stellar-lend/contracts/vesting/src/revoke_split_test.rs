#[cfg(test)]
mod tests {
    use crate::{VestingContract, VestingError};

    /// Revoking mid-vest must correctly split tokens:
    /// 1. Already-vested amount remains accessible to the grantee via claim.
    /// 2. Unvested remainder is clawed back to the treasury.
    /// 3. claimed + clawed_back == original_principal.
    #[test]
    fn test_revoke_mid_vest_split_accuracy() {
        let mut c = VestingContract::new("admin", "treasury");
        // 1_000 tokens, starts at t=0, duration=1_000 s, no cliff.
        c.add_grant("admin", "alice", 1_000, 0, 1_000, 0).unwrap();

        // Revoke at t=300: 300 tokens vested, 700 unvested.
        let clawed = c.revoke("admin", "alice", 300).expect("revoke failed");
        assert_eq!(clawed, 700, "treasury should receive unvested 700");
        assert_eq!(c.balance_of("treasury"), 700);

        // Alice can still claim the 300 vested tokens.
        let claimed = c.claim("alice", 300).expect("claim after revoke failed");
        assert_eq!(claimed, 300);
        assert_eq!(c.balance_of("alice"), 300);

        // Conservation: claimed + clawed == original principal.
        assert_eq!(claimed + clawed, 1_000);
        assert_eq!(c.total_locked(), 0);
    }

    #[test]
    fn revoke_non_existent_grant_returns_no_such_grant() {
        let mut c = VestingContract::new("admin", "treasury");
        let err = c.revoke("admin", "nobody", 0).unwrap_err();
        assert_eq!(err, VestingError::NoSuchGrant);
    }
}
