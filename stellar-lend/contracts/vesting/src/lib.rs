use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub grantee: String,
    pub total: u128,
    pub claimed: u128,
    pub released: u128,
    pub start_seconds: u64,
    pub duration_seconds: u64,
    pub cliff_seconds: u64,
    pub revoked: bool,
}

impl Grant {
    pub fn vested_at(&self, now: u64) -> u128 {
        if now < self.start_seconds + self.cliff_seconds {
            return 0;
        }
        if self.duration_seconds == 0 {
            return self.total;
        }
        let end = self.start_seconds.saturating_add(self.duration_seconds);
        let effective = if now >= end { end } else { now };
        if effective <= self.start_seconds {
            return 0;
        }
        let elapsed = effective - self.start_seconds;
        (self.total as u128 * elapsed as u128) / self.duration_seconds as u128
    }

    pub fn claimable(&self) -> u128 {
        self.released.saturating_sub(self.claimed)
    }

    fn sync(&mut self, now: u64) -> u128 {
        let vested = self.vested_at(now);
        let newly_released = vested.saturating_sub(self.released);
        self.released = vested;
        newly_released
    }

    fn locked(&self) -> u128 {
        self.total.saturating_sub(self.released)
    }
}

pub struct VestingContract {
    pub admin: String,
    pub treasury: String,
    grants: HashMap<String, Vec<Grant>>,
    pub balances: HashMap<String, u128>,
    total_locked: u128,
}

impl VestingContract {
    pub fn new(admin: &str, treasury: &str) -> Self {
        Self {
            admin: admin.to_string(),
            treasury: treasury.to_string(),
            grants: HashMap::new(),
            balances: HashMap::new(),
            total_locked: 0,
        }
    }

    /// Adds a vesting schedule for `grantee` and increases the aggregate locked supply.
    pub fn add_grant(
        &mut self,
        grantee: &str,
        total: u128,
        start_seconds: u64,
        duration_seconds: u64,
        cliff_seconds: u64,
    ) {
        let g = Grant {
            grantee: grantee.to_string(),
            total,
            claimed: 0,
            released: 0,
            start_seconds,
            duration_seconds,
            cliff_seconds,
            revoked: false,
        };
        self.grants.entry(grantee.to_string()).or_default().push(g);
        let bal = self.balances.entry("contract".to_string()).or_default();
        *bal += total;
        self.total_locked += total;
    }

    fn sync_grants(&mut self, grantee: &str, now: u64) {
        if let Some(grants) = self.grants.get_mut(grantee) {
            for grant in grants.iter_mut() {
                let newly_released = grant.sync(now);
                self.total_locked = self.total_locked.saturating_sub(newly_released);
            }
        }
    }

    pub fn claim(&mut self, grantee: &str, now: u64) -> u128 {
        self.sync_grants(grantee, now);
        let grants = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return 0,
        };

        let mut amount = 0;
        for grant in grants.iter_mut() {
            if grant.revoked {
                continue;
            }
            let claimable = grant.claimable();
            grant.claimed += claimable;
            amount += claimable;
        }

        if amount == 0 {
            return 0;
        }

        let cbal = self.balances.entry("contract".to_string()).or_default();
        if *cbal >= amount {
            *cbal -= amount;
            let gbal = self.balances.entry(grantee.to_string()).or_default();
            *gbal += amount;
        }
        amount
    }

    pub fn revoke(&mut self, caller: &str, grantee: &str, now: u64) -> Result<u128, String> {
        if caller != self.admin {
            return Err("only admin can revoke".to_string());
        }

        self.sync_grants(grantee, now);
        let grants = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return Err("no such grant".to_string()),
        };

        let mut transfer = 0;
        let mut revoked_any = false;
        for grant in grants.iter_mut() {
            if grant.revoked {
                continue;
            }
            revoked_any = true;
            let unvested = grant.locked();
            transfer += unvested;
            self.total_locked = self.total_locked.saturating_sub(unvested);
            grant.total = grant.released;
            grant.revoked = true;
        }

        if !revoked_any {
            return Err("already revoked".to_string());
        }

        let cbal = self.balances.entry("contract".to_string()).or_default();
        let actual_transfer = if *cbal >= transfer { transfer } else { *cbal };
        *cbal = cbal.saturating_sub(actual_transfer);
        let tbal = self.balances.entry(self.treasury.clone()).or_default();
        *tbal += actual_transfer;
        Ok(actual_transfer)
    }

    /// Returns the current token balance recorded for `who`.
    pub fn balance_of(&self, who: &str) -> u128 {
        *self.balances.get(who).unwrap_or(&0)
    }

    /// Returns every vesting schedule recorded for `grantee`.
    pub fn get_grants(&self, grantee: &str) -> Vec<Grant> {
        self.grants.get(grantee).cloned().unwrap_or_default()
    }

    /// Returns the aggregate locked supply tracked across all grants.
    pub fn total_locked(&self) -> u128 {
        self.total_locked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_before_cliff_is_zero() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("alice", 1000, 1000, 1000, 200);
        let claimed = c.claim("alice", 1100);
        assert_eq!(claimed, 0);
        assert_eq!(c.balance_of("alice"), 0);
        assert_eq!(c.total_locked(), 1000);
    }

    #[test]
    fn claim_after_cliff_partial() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("bob", 1000, 1000, 1000, 100);
        let claimed = c.claim("bob", 1200);
        assert_eq!(claimed, 200);
        assert_eq!(c.balance_of("bob"), 200);
        assert_eq!(c.total_locked(), 800);
    }

    #[test]
    fn revoke_claws_unvested_to_treasury() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("carol", 1000, 1000, 1000, 100);
        let _ = c.claim("carol", 1200);
        assert_eq!(c.balance_of("contract"), 800);
        let transferred = c.revoke("admin", "carol", 1200).expect("revoke failed");
        assert_eq!(transferred, 800);
        assert_eq!(c.balance_of("treasury"), 800);
        assert_eq!(c.claim("carol", 1300), 0);
        assert_eq!(c.total_locked(), 0);
    }

    #[test]
    fn revoke_only_admin() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("dan", 500, 0, 100, 0);
        let res = c.revoke("not-admin", "dan", 10);
        assert!(res.is_err());
        assert_eq!(c.total_locked(), 500);
    }
}

#[cfg(test)]
mod vesting_views_test;
