use anyhow::{anyhow, Result};
use bincode;
use ed25519_dalek::{PublicKey, Signature, Verifier};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Typed contract errors to represent specific domain violations.
#[derive(Debug, PartialEq, Eq)]
pub enum BridgeError {
    /// Emitted when attempting to configure a rolling window of length 0.
    InvalidWindowSize,
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::InvalidWindowSize => write!(f, "InvalidWindowSize: window_size must be > 0"),
        }
    }
}

impl std::error::Error for BridgeError {}
/// Store validator public keys as raw bytes so the struct remains serde-friendly
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorSet {
    pub validators: Vec<Vec<u8>>, // each is PublicKey::to_bytes()
}

impl ValidatorSet {
    /// Returns the effective validator count used for quorum decisions.
    ///
    /// Duplicate byte-encoded keys collapse to a single logical validator so a
    /// malformed set cannot silently raise the quorum threshold by repeating the
    /// same public key multiple times.
    pub fn len(&self) -> usize {
        self.validators
            .iter()
            .map(|validator| validator.as_slice())
            .collect::<HashSet<_>>()
            .len()
    }

    /// Returns the strict supermajority quorum threshold for this set.
    ///
    /// The threshold is computed from the deduplicated validator count exposed
    /// by [`ValidatorSet::len`], so repeated keys never inflate the required
    /// number of unique signatures.
    pub fn threshold(&self) -> usize {
        // Supermajority: > 2/3 of validators
        let n = self.len();
        (n * 2) / 3 + 1
    }

    /// Returns `true` when `pk` is present anywhere in the raw validator list.
    pub fn contains_pk(&self, pk: &PublicKey) -> bool {
        let b = pk.to_bytes();
        self.validators.iter().any(|v| v.as_slice() == b.as_ref())
    }

    /// Returns the raw byte-encoded validator list in storage order.
    pub fn to_bytes_vec(&self) -> Vec<Vec<u8>> {
        self.validators.clone()
    }
}

#[derive(Clone, Debug)]
pub struct Bridge {
    pub epoch: u64,
    pub validators: ValidatorSet,
    /// Maximum cumulative inbound value (in the bridge's native token unit,
    /// matching the `i128` amount convention used elsewhere in this workspace)
    /// that may be admitted within a single rolling window.
    ///
    /// A value of `0` means "no inbound" (fail-closed) — not "unlimited".
    /// Defaults to `0` so a freshly constructed `Bridge` admits no inbound
    /// value until an operator explicitly opts in via [`Bridge::set_inbound_cap`].
    pub max_per_window: i128,
    /// Length of the rolling inbound-value window, in ledger-time seconds
    /// (e.g. `86_400` for a calendar-day window). Must be > 0 once configured.
    pub window_size: u64,
    /// Ledger time at which the current window began.
    pub window_start: u64,
    /// Cumulative inbound value admitted so far within `[window_start, window_start + window_size)`.
    pub window_inbound_total: i128,
}

/// Default rolling window length: one day, in seconds.
pub const DEFAULT_INBOUND_WINDOW_SECS: u64 = 86_400;

impl Bridge {
    /// Construct a new bridge. Inbound value transfer is **fail-closed by
    /// default**: `max_per_window` starts at `0`, so [`Bridge::admit_inbound`]
    /// rejects everything until [`Bridge::set_inbound_cap`] is called with a
    /// non-zero cap.
    pub fn new(initial: ValidatorSet) -> Self {
        Bridge {
            epoch: 0,
            validators: initial,
            max_per_window: 0,
            window_size: DEFAULT_INBOUND_WINDOW_SECS,
            window_start: 0,
            window_inbound_total: 0,
        }
    }

    /// Verify a quorum proof from the current validator set over the (new_set, epoch) payload
    fn verify_quorum_proof(&self, new_set: &ValidatorSet, epoch: u64, proofs: &[(PublicKey, Signature)]) -> Result<()> {
        if proofs.is_empty() {
            return Err(anyhow!("empty proofs"));
        }

        // payload to be signed: bincode(new_set_bytes_vec, epoch)
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch))?;

        let mut unique_signers: HashSet<Vec<u8>> = HashSet::new();
        for (pk, sig) in proofs.iter() {
            // signer must be part of the current validator set
            if !self.validators.contains_pk(pk) {
                return Err(anyhow!("proof contains signer not in current validator set"));
            }

            // avoid double counting
            let key_bytes = pk.to_bytes().to_vec();
            if unique_signers.contains(&key_bytes) {
                continue;
            }

            // verify signature
            pk.verify(&payload, sig).map_err(|e| anyhow!(e.to_string()))?;
            unique_signers.insert(key_bytes);
        }

        if unique_signers.len() < self.validators.threshold() {
            return Err(anyhow!("insufficient quorum in proofs"));
        }

        Ok(())
    }

    /// Rotate validators to `new_set` at `next_epoch` if `proofs` from current set form a quorum.
    /// The `epoch` must be exactly current_epoch + 1.
    pub fn rotate_validators(&mut self, new_set: ValidatorSet, epoch: u64, proofs: Vec<(PublicKey, Signature)>) -> Result<()> {
        if epoch != self.epoch + 1 {
            return Err(anyhow!("invalid epoch: must be current_epoch + 1"));
        }

        self.verify_quorum_proof(&new_set, epoch, &proofs)?;

        // swap atomically
        self.validators = new_set;
        self.epoch = epoch;
        Ok(())
    }

    /// Verify inbound message signature epoch. Messages signed with an epoch < current epoch are rejected.
    pub fn validate_inbound_epoch(&self, signed_epoch: u64) -> Result<()> {
        if signed_epoch < self.epoch {
            return Err(anyhow!("message signed by retired validator set (epoch too old)"));
        }
        Ok(())
    }

    /// Reconfigure the per-window inbound value cap and (re)start the
    /// window fresh at `current_time`.
    ///
    /// `max_per_window == 0` is a valid, intentional configuration meaning
    /// "no inbound" (fail-closed) — use a positive value to actually permit
    /// inbound transfers. `window_size` must be greater than zero.
    pub fn set_inbound_cap(&mut self, max_per_window: i128, window_size: u64, current_time: u64) -> Result<()> {
        if max_per_window < 0 {
            return Err(anyhow!("max_per_window must be >= 0"));
        }
        if window_size == 0 {
            return Err(BridgeError::InvalidWindowSize.into());
        }

        self.max_per_window = max_per_window;
        self.window_size = window_size;
        self.window_start = current_time;
        self.window_inbound_total = 0;
        Ok(())
    }

    /// Roll the inbound-value window forward if `current_time` has moved
    /// past the end of the current window. Resetting realigns the window to
    /// start at `current_time` rather than stepping forward in fixed
    /// `window_size` increments, so a bridge that sat idle for a long time
    /// doesn't pay for that idle period with a stale, partially-consumed
    /// window (see SECURITY_NOTES.md for the rationale).
    fn roll_window_if_expired(&mut self, current_time: u64) {
        if current_time < self.window_start {
            // Guard against non-monotonic clock adjustments (time moving backwards).
            return;
        }

        if let Some(window_end) = self.window_start.checked_add(self.window_size) {
            if current_time >= window_end {
                self.window_start = current_time;
                self.window_inbound_total = 0;
            }
        }
    }

    /// Admit an inbound transfer of `amount` against the per-window inbound
    /// value cap, tracked on rolling ledger time (not block count).
    ///
    /// Rejects (without mutating any state) if:
    /// - `amount` is negative,
    /// - the cap is configured as `0` (fail-closed — no inbound permitted
    ///   regardless of amount), or
    /// - admitting `amount` would push the window's cumulative inbound value
    ///   above `max_per_window`.
    ///
    /// On success, `amount` is added to the current window's running total
    /// and `Ok(())` is returned. Callers are expected to have already
    /// validated validator quorum and inbound epoch separately — this check
    /// is purely the value-cap defense-in-depth layer.
    pub fn admit_inbound(&mut self, amount: i128, current_time: u64) -> Result<()> {
        if amount < 0 {
            return Err(anyhow!("inbound amount must be >= 0"));
        }

        if self.max_per_window == 0 {
            return Err(anyhow!("inbound cap is zero (fail-closed): no inbound transfers permitted"));
        }

        self.roll_window_if_expired(current_time);

        let new_total = self
            .window_inbound_total
            .checked_add(amount)
            .ok_or_else(|| anyhow!("inbound window total overflow"))?;

        if new_total > self.max_per_window {
            return Err(anyhow!("inbound cap exceeded for current window"));
        }

        self.window_inbound_total = new_total;
        Ok(())
    }
}

#[cfg(test)]
mod rotation_test;

#[cfg(test)]
mod inbound_cap_test;

#[cfg(test)]
mod epoch_monotonicity_proptest;

#[cfg(test)]
mod window_guard_test;

#[cfg(test)]
mod validatorset_proptest;

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Keypair, Signer};
    use rand::rngs::OsRng;

    fn make_keypairs(n: usize) -> Vec<Keypair> {
        let mut rng = OsRng;
        (0..n).map(|_| Keypair::generate(&mut rng)).collect()
    }

    #[test]
    fn test_rotate_success_and_epoch_boundary() {
        // initial set A: 4 validators
        let kp_a = make_keypairs(4);
        let a_pks: Vec<PublicKey> = kp_a.iter().map(|k| k.public).collect();
        let initial = ValidatorSet { validators: a_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };
        let mut bridge = Bridge::new(initial);

        // new set B: 3 validators
        let kp_b = make_keypairs(3);
        let b_pks: Vec<PublicKey> = kp_b.iter().map(|k| k.public).collect();
        let new_set = ValidatorSet { validators: b_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };

        // proofs: have >2/3 of A sign the (new_set, epoch=1) payload
        let epoch = 1u64;
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch)).unwrap();

        // need threshold of A: (4*2)/3+1 = 3
        let mut proofs = vec![];
        for i in 0..3 {
            let sig = kp_a[i].sign(&payload);
            proofs.push((kp_a[i].public, sig));
        }

        // rotate should succeed
        bridge.rotate_validators(new_set.clone(), epoch, proofs).expect("rotation failed");
        assert_eq!(bridge.epoch, 1);

        // messages signed with epoch 0 should be rejected
        assert!(bridge.validate_inbound_epoch(0).is_err());
        // messages signed with epoch 1 are accepted
        assert!(bridge.validate_inbound_epoch(1).is_ok());
        assert!(bridge.validate_inbound_epoch(2).is_ok(), "future epochs allowed by this check (policy dependent)");
    }

    #[test]
    fn test_rotate_reject_insufficient_quorum() {
        let kp_a = make_keypairs(5);
        let a_pks: Vec<PublicKey> = kp_a.iter().map(|k| k.public).collect();
        let initial = ValidatorSet { validators: a_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };
        let mut bridge = Bridge::new(initial);

        let kp_b = make_keypairs(3);
        let b_pks: Vec<PublicKey> = kp_b.iter().map(|k| k.public).collect();
        let new_set = ValidatorSet { validators: b_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };

        let epoch = 1u64;
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch)).unwrap();

        // need threshold of A: (5*2)/3+1 = 4. Provide only 3 signatures => fail
        let mut proofs = vec![];
        for i in 0..3 {
            let sig = kp_a[i].sign(&payload);
            proofs.push((kp_a[i].public, sig));
        }

        assert!(bridge.rotate_validators(new_set, epoch, proofs).is_err());
    }

    #[test]
    fn test_rotate_reject_wrong_epoch() {
        let kp_a = make_keypairs(3);
        let a_pks: Vec<PublicKey> = kp_a.iter().map(|k| k.public).collect();
        let initial = ValidatorSet { validators: a_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };
        let mut bridge = Bridge::new(initial);

        let kp_b = make_keypairs(2);
        let b_pks: Vec<PublicKey> = kp_b.iter().map(|k| k.public).collect();
        let new_set = ValidatorSet { validators: b_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };

        // wrong epoch (must be 1)
        let epoch = 2u64;
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch)).unwrap();

        let mut proofs = vec![];
        for i in 0..2 {
            let sig = kp_a[i].sign(&payload);
            proofs.push((kp_a[i].public, sig));
        }

        assert!(bridge.rotate_validators(new_set, epoch, proofs).is_err());
    }
}
