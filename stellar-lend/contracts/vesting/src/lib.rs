use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Grant {
    pub grantee: String,
    pub total: u128,
    pub claimed: u128,
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
        // linear vesting proportion: total * elapsed / duration
        (self.total as u128 * elapsed as u128) / self.duration_seconds as u128
    }

    pub fn claimable_at(&self, now: u64) -> u128 {
        if self.revoked {
            return 0;
        }
        let vested = self.vested_at(now);
        if vested <= self.claimed {
            0
        } else {
            vested - self.claimed
        }
    }
}

pub struct VestingContract {
    pub admin: String,
    pub treasury: String,
    grants: HashMap<String, Grant>,
    pub balances: HashMap<String, u128>,
}

impl VestingContract {
    pub fn new(admin: &str, treasury: &str) -> Self {
        Self {
            admin: admin.to_string(),
            treasury: treasury.to_string(),
            grants: HashMap::new(),
            balances: HashMap::new(),
        }
    }

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
            start_seconds,
            duration_seconds,
            cliff_seconds,
            revoked: false,
        };
        self.grants.insert(grantee.to_string(), g);
        // escrow total in contract balance
        let bal = self.balances.entry("contract".to_string()).or_default();
        *bal += total;
    }

    pub fn claim(&mut self, grantee: &str, now: u64) -> u128 {
        let g = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return 0,
        };
        let amount = g.claimable_at(now);
        if amount == 0 {
            return 0;
        }
        g.claimed += amount;
        // transfer from contract balance to grantee
        let cbal = self.balances.entry("contract".to_string()).or_default();
        if *cbal >= amount {
            *cbal -= amount;
            let gbal = self.balances.entry(g.grantee.clone()).or_default();
            *gbal += amount;
        } else {
            // insufficient balance; treat as zero transfer
        }
        amount
    }

    pub fn revoke(&mut self, caller: &str, grantee: &str, now: u64) -> Result<u128, String> {
        if caller != self.admin {
            return Err("only admin can revoke".to_string());
        }
        let g = match self.grants.get_mut(grantee) {
            Some(x) => x,
            None => return Err("no such grant".to_string()),
        };
        if g.revoked {
            return Err("already revoked".to_string());
        }
        let vested = g.vested_at(now);
        let unvested = if g.total > vested {
            g.total - vested
        } else {
            0
        };
        // reduce contract balance and send unvested to treasury
        let cbal = self.balances.entry("contract".to_string()).or_default();
        let transfer = if *cbal >= unvested { unvested } else { *cbal };
        *cbal = cbal.saturating_sub(transfer);
        let tbal = self.balances.entry(self.treasury.clone()).or_default();
        *tbal += transfer;
        // mark revoked and reduce grant total to vested amount
        g.revoked = true;
        g.total = vested;
        Ok(transfer)
    }

    // helpers for tests
    pub fn balance_of(&self, who: &str) -> u128 {
        *self.balances.get(who).unwrap_or(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_before_cliff_is_zero() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("alice", 1000, 1000, 1000, 200); // start=1000, cliff=200
                                                     // time before cliff
        let claimed = c.claim("alice", 1100); // start+100 < start+cliff
        assert_eq!(claimed, 0);
        assert_eq!(c.balance_of("alice"), 0);
    }

    #[test]
    fn claim_after_cliff_partial() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("bob", 1000, 1000, 1000, 100); // cliff at 1100
                                                   // at time 1200, elapsed since start = 200, vested = 1000 * 200/1000 = 200
        let claimed = c.claim("bob", 1200);
        assert_eq!(claimed, 200);
        assert_eq!(c.balance_of("bob"), 200);
    }

    #[test]
    fn revoke_claws_unvested_to_treasury() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("carol", 1000, 1000, 1000, 100);
        // at time 1200 vested=200
        let _ = c.claim("carol", 1200);
        // contract had 1000, after claim it has 800
        assert_eq!(c.balance_of("contract"), 800);
        // revoke by admin at time 1200
        let transferred = c.revoke("admin", "carol", 1200).expect("revoke failed");
        // unvested was 800, so treasury should get 800
        assert_eq!(transferred, 800);
        assert_eq!(c.balance_of("treasury"), 800);
        // grantee should only have vested (200) and further claims are zero
        assert_eq!(c.claim("carol", 1300), 0);
    }

    #[test]
    fn revoke_only_admin() {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("dan", 500, 0, 100, 0);
        let res = c.revoke("not-admin", "dan", 10);
        assert!(res.is_err());
    }
}
