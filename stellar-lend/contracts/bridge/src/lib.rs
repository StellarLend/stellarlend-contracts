#![no_std]
use soroban_sdk::{contract, contractimpl, contracterror, contracttype, Env, Map};

/// Error codes for Bridge contract operations.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeError {
    /// Nonce overflow: destination nonce has reached u64::MAX and cannot be incremented.
    NonceOverflow = 1,
}

/// Ledger storage key for the outbound nonce map.
#[contracttype]
pub enum BridgeDataKey {
    /// Maps destination network ID (u32) to its next outbound nonce (u64).
    OutboundNonces,
}

/// Emitted when an outbound bridge message is created.
/// Carries the destination network and the nonce assigned to this message,
/// giving relayers and the destination chain a unique, ordered identity.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundMessageEvent {
    /// Destination network identifier.
    pub dest: u32,
    /// Monotonically increasing nonce for this destination.
    pub nonce: u64,
}

/// Bridge contract with per-destination outbound nonce sequencing.
///
/// Each outbound transfer is assigned a strictly-increasing nonce keyed by
/// destination network. This gives relayers and downstream chains a
/// replay-resistant, deterministically ordered message identity.
#[contract]
pub struct Bridge;

#[contractimpl]
impl Bridge {
    /// Retrieve the current outbound nonce map from storage, or return an empty map.
    fn load_nonces(env: &Env) -> Map<u32, u64> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Map<u32, u64>>(&BridgeDataKey::OutboundNonces)
            .unwrap_or_else(|| Map::new(env))
    }

    /// Persist the outbound nonce map to storage.
    fn save_nonces(env: &Env, nonces: &Map<u32, u64>) {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::OutboundNonces, nonces);
    }

    /// Return the next outbound nonce for `dest`, then increment it.
    ///
    /// The first call for a fresh destination returns `0`.
    /// Subsequent calls return strictly increasing values.
    /// Panics with `BridgeError::NonceOverflow` if the nonce would exceed `u64::MAX`.
    ///
    /// # Arguments
    /// * `dest` - Destination network identifier (u32).
    ///
    /// # Returns
    /// The nonce assigned to this outbound message.
    pub fn next_outbound_nonce(env: Env, dest: u32) -> Result<u64, BridgeError> {
        let mut nonces = Self::load_nonces(&env);
        let current = nonces.get(dest).unwrap_or(0u64);
        let next = current.checked_add(1).ok_or(BridgeError::NonceOverflow)?;
        nonces.set(dest, next);
        Self::save_nonces(&env, &nonces);

        // Emit outbound event so relayers can track the message identity.
        env.events().publish(
            (soroban_sdk::symbol_short!("outbound"),),
            OutboundMessageEvent {
                dest,
                nonce: current,
            },
        );

        Ok(current)
    }

    /// Return the next nonce that will be assigned for `dest` without incrementing.
    ///
    /// Returns `0` if no messages have been sent to `dest` yet.
    ///
    /// # Arguments
    /// * `dest` - Destination network identifier (u32).
    ///
    /// # Returns
    /// The nonce that the next `next_outbound_nonce` call will return for this destination.
    pub fn peek_outbound_nonce(env: Env, dest: u32) -> u64 {
        let nonces = Self::load_nonces(&env);
        nonces.get(dest).unwrap_or(0u64)
    }

    /// Returns the number of active (non-paused) validators.
    ///
    /// "Active" excludes any validator whose byte-encoded public key is in
    /// [`Bridge::paused_validators`]. Duplicate keys in the raw validator
    /// list still collapse to one logical validator, matching
    /// [`ValidatorSet::len`] semantics.
    pub fn active_validator_count(&self) -> usize {
        self.validators
            .validators
            .iter()
            .filter(|v| !self.paused_validators.contains(*v))
            .map(|v| v.as_slice())
            .collect::<HashSet<_>>()
            .len()
    }

    /// Effective supermajority quorum threshold computed from the active
    /// (non-paused) validator count.
    ///
    /// If every validator is paused, this returns `1` — the same value
    /// [`ValidatorSet::threshold`] returns for an empty set. This is a
    /// documented edge case: a fully-paused bridge is mathematically
    /// unreachable (no active signer can ever meet any threshold > 0) and
    /// pause / unpause calls will reject based on the fail-closed
    /// arithmetic before this returns in any realistic configuration. See
    /// [`BridgeError::PauseWouldBreakQuorum`] for the guard that prevents
    /// the bridge from getting into this state in the first place.
    pub fn effective_threshold(&self) -> usize {
        let n = self.active_validator_count();
        (n * 2) / 3 + 1
    }

    /// Returns `true` iff `pk`'s byte encoding is in the paused set.
    ///
    /// This is a pure membership check; it does **not** validate that the
    /// validator is also part of the current validator set. To check both
    /// conditions, see [`Bridge::is_active_validator`].
    pub fn is_paused(&self, pk: &PublicKey) -> bool {
        self.paused_validators.contains(&pk.to_bytes().to_vec())
    }

    /// Returns `true` iff `pk` is currently part of the validator set
    /// **and** is not paused — i.e. its signature counts toward quorum.
    pub fn is_active_validator(&self, pk: &PublicKey) -> bool {
        self.validators.contains_pk(pk) && !self.is_paused(pk)
    }

    /// Returns the raw byte-encoding of every currently-paused validator,
    /// in arbitrary set-iteration order. Useful for audit / introspection
    /// tooling.
    pub fn paused_list(&self) -> Vec<Vec<u8>> {
        self.paused_validators.iter().cloned().collect()
    }

    /// Builds the canonical, domain-separated payload for a validator-set
    /// rotation quorum proof.
    ///
    /// ```text
    ///   payload = bincode((QUORUM_PROOF_DOMAIN, bridge_id, new_set_bytes, epoch))
    /// ```
    ///
    /// `bridge_id` identifies the deployment, `new_set_bytes` preserves the
    /// proposed set's storage order, and `epoch` is the exact successor epoch.
    /// Current active validators must all sign the identical bytes returned by
    /// this function; changing any field invalidates the signature.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload cannot be serialized.
    pub fn quorum_proof_payload(
        bridge_id: &[u8],
        new_set: &ValidatorSet,
        epoch: u64,
    ) -> Result<Vec<u8>> {
        Ok(bincode::serialize(&(
            QUORUM_PROOF_DOMAIN,
            bridge_id,
            new_set.to_bytes_vec(),
            epoch,
        ))?)
    }

    /// Verifies that current active validators authorized `new_set` for
    /// `epoch` with a strict supermajority quorum proof.
    ///
    /// Paused validator signatures are *silently skipped* — they are neither
    /// verified nor counted toward the quorum, and they do not cause the
    /// overall proof to fail.
    ///
    /// The supplied proof vector is attacker-influenced, so it is bounded before
    /// any signature verification work runs. A proof cannot contain more
    /// entries than there are unique validators in the current set, and each
    /// signer public key may appear at most once. These checks cap verifier
    /// work at O(current validator set size) and reject duplicate-laden proofs
    /// before they can burn CPU on redundant signature checks.
    fn verify_quorum_proof(
        &self,
        new_set: &ValidatorSet,
        epoch: u64,
        proofs: &[(PublicKey, Signature)],
    ) -> Result<()> {
        if proofs.is_empty() {
            return Err(anyhow!("empty proofs"));
        }

        let max_proofs = self.validators.len();
        if proofs.len() > max_proofs {
            return Err(anyhow!(
                "quorum proof has {} entries but current validator set has {} unique validators",
                proofs.len(),
                max_proofs
            ));
        }

        let mut seen_proof_signers: HashSet<Vec<u8>> = HashSet::new();
        for (pk, _) in proofs.iter() {
            let key_bytes = pk.to_bytes().to_vec();
            if !seen_proof_signers.insert(key_bytes) {
                return Err(anyhow!("quorum proof contains duplicate signer"));
            }
        }

        // Domain-separated payload: bincode(domain_tag, bridge_id, new_set_bytes, epoch).
        // The constant tag + per-instance bridge_id bind every signature to this
        // exact bridge instance and purpose, preventing cross-context reuse (#1146).
        let payload = Self::quorum_proof_payload(&self.bridge_id, new_set, epoch)?;

        let mut unique_active_signers: HashSet<Vec<u8>> = HashSet::new();
        for (pk, sig) in proofs.iter() {
            // Signer must be part of the current validator set. This applies
            // to paused validators, too — paused keys must still be in the
            // current set; otherwise they should have been rotated out.
            if !self.validators.contains_pk(pk) {
                return Err(anyhow!("proof contains signer not in current validator set"));
            }

            // Paused validators are silently skipped. They do not count
            // toward the quorum, and we do not verify their signature
            // (the key is presumed compromised, so its signature carries no
            // trust weight; verifying it is wasted work, and a malformed
            // signature from a compromised-but-paused key should not bring
            // down the rest of the proof).
            let key_bytes = pk.to_bytes().to_vec();
            if self.paused_validators.contains(&key_bytes) {
                continue;
            }

            pk.verify(&payload, sig).map_err(|e| anyhow!(e.to_string()))?;
            unique_active_signers.insert(key_bytes);
        }

        if unique_active_signers.len() < self.effective_threshold() {
            return Err(anyhow!("insufficient quorum in proofs"));
        }

        Ok(())
    }

    /// Set the maximum validator-set churn limit allowed per rotation.
    ///
    /// If `max_churn` is `None`, the churn limit is disabled.
    pub fn set_max_churn(&mut self, max_churn: Option<u32>) {
        self.max_churn = max_churn;
    }

    /// Applies a validator-set rotation authorized by the current active set.
    ///
    /// `epoch` must equal `self.epoch + 1`. `proofs` must contain a strict
    /// supermajority of unique valid signatures from current active validators
    /// over the canonical `(domain, bridge_id, new_set, epoch)` payload.
    ///
    /// # Security validation
    ///
    /// Before verifying the quorum proof, this function validates the incoming
    /// `new_set`:
    ///
    /// 1. **Size bounds** — the deduplicated validator count must lie within
    ///    [`MIN_VALIDATORS`, `MAX_VALIDATORS`].  Rejects empty or single-validator
    ///    sets that would collapse the supermajority into a single point of
    ///    failure, and oversized sets that would make quorum verification
    ///    prohibitively expensive.
    /// 2. **Duplicate keys** — the raw `new_set` must not contain duplicate
    ///    public-key byte representations.  While the [`ValidatorSet::len`] and
    ///    [`ValidatorSet::threshold`] methods themselves deduplicate for quorum
    ///    counting, a set that *relies* on dedup to meet its size bound is a
    ///    bug waiting to happen — the extra duplicate entries serve no purpose
    ///    and may mask an operator error during key collection.
    ///
    /// The paused-validator set is cleared on rotation: pauses are scoped to
    /// the compromised key material in the *current* set, and the *new* set
    /// implies fresh, unpaused keys by default. If a key from the old set
    /// happens to also be present in the new set, that's a configuration
    /// choice the operator must make explicitly via a subsequent
    /// [`Bridge::pause_validator`] call.
    ///
    /// Also enforces the `max_churn` limit (if configured) on the symmetric difference
    /// between the current validator set and the new validator set.
    ///
    /// On success, the validator set and epoch advance atomically and stale
    /// pause flags are cleared. On failure, those fields remain unchanged.
    ///
    /// # Returns
    ///
    /// Returns the symmetric-difference churn count between the old and new
    /// validator sets.
    ///
    /// # Errors
    ///
    /// Rejects a non-successor epoch, an out-of-bounds or duplicate-key set, a
    /// churn-limit violation, or an invalid/insufficient quorum proof.
    pub fn rotate_validators(
        &mut self,
        new_set: ValidatorSet,
        epoch: u64,
        proofs: Vec<(PublicKey, Signature)>,
    ) -> Result<u32> {
        if epoch != self.epoch + 1 {
            return Err(anyhow!("invalid epoch: must be current_epoch + 1"));
        }

        // Reject raw duplicate keys before size checks so callers get the
        // most specific error for malformed validator-set input.
        {
            let mut seen = std::collections::HashSet::new();
            for key_bytes in &new_set.validators {
                if !seen.insert(key_bytes.as_slice()) {
                    return Err(anyhow!("{}", BridgeError::DuplicateValidatorKey));
                }
            }
        }

        // ── Validate new_set size bounds ──────────────────────────────────
        let unique_count = new_set.len();
        if unique_count < MIN_VALIDATORS {
            return Err(anyhow!("{}", BridgeError::ValidatorSetTooSmall));
        }
        if unique_count > MAX_VALIDATORS {
            return Err(anyhow!("{}", BridgeError::ValidatorSetTooLarge));
        }

        // Compute churn: symmetric difference size between current set and new set.
        let current_set_uniq: HashSet<&[u8]> = self
            .validators
            .validators
            .iter()
            .map(|v| v.as_slice())
            .collect();
        let new_set_uniq: HashSet<&[u8]> = new_set
            .validators
            .iter()
            .map(|v| v.as_slice())
            .collect();

        let added = new_set_uniq.difference(&current_set_uniq).count();
        let removed = current_set_uniq.difference(&new_set_uniq).count();

        let added_u32 = u32::try_from(added).map_err(|_| anyhow!("added count overflow"))?;
        let removed_u32 = u32::try_from(removed).map_err(|_| anyhow!("removed count overflow"))?;
        let churn = added_u32
            .checked_add(removed_u32)
            .ok_or_else(|| anyhow!("churn computation overflowed"))?;

        if let Some(limit) = self.max_churn {
            if churn > limit {
                return Err(anyhow!(
                    "validator set churn of {} exceeds the limit of {}",
                    churn,
                    limit
                ));
            }
        }

        self.verify_quorum_proof(&new_set, epoch, &proofs)?;

        // swap atomically
        self.validators = new_set;
        self.epoch = epoch;
        // stale pause flags belong to the old (rotated-out) key material; clear.
        self.paused_validators.clear();
        Ok(churn)
    }

    /// Guardian-gated pause of a single validator.
    ///
    /// On success the validator is added to [`Bridge::paused_validators`] and
    /// a [`ValidatorEvent::Paused`] event is returned for the caller to log
    /// or persist. The validator's signature is ignored in subsequent
    /// `verify_quorum_proof` calls, and the effective quorum threshold is
    /// recomputed against the remaining active validators.
    ///
    /// ### Fail-closed guard
    ///
    /// The *supplied* signature is verified against the configured
    /// [`Bridge::guardian`] (not against `validator`) over the action-bound
    /// payload `"BRIDGE_PAUSE:" || validator.to_bytes()`. This binds the
    /// authorisation to a specific (action, target_validator) pair so a
    /// pause signature cannot be replayed as an unpause signature, and vice
    /// versa (the inverse tag `"BRIDGE_UNPAUSE:"` is used for unpauses).
    ///
    /// Pausing is rejected with [`BridgeError::PauseWouldBreakQuorum`] if it
    /// would leave the active validator count below the new effective
    /// supermajority threshold (so a quorum-proof could never reach the new
    /// threshold). This protects the bridge from being frozen by an overly
    /// aggressive guardian and is enforced upstream of the signature check
    /// so a malicious caller cannot burn the guardian's signature on a
    /// request that would have been rejected anyway.
    pub fn pause_validator(
        &mut self,
        validator: &PublicKey,
        signature: &Signature,
    ) -> Result<ValidatorEvent> {
        // 1. Guardian must be configured.
        let guardian = self.guardian.ok_or(BridgeError::NoGuardianConfigured)?;

        let v_bytes = validator.to_bytes().to_vec();

        // 2. Target must be part of the current validator set.
        if !self.validators.contains_pk(validator) {
            return Err(BridgeError::UnknownValidator.into());
        }

        // 3. Reject double-pause explicitly *before* the fail-closed math,
        //    so a re-pause attempt returns the precise `AlreadyPaused`
        //    diagnostic instead of `PauseWouldBreakQuorum` (whose math is
        //    only meaningful when the target is currently active).
        if self.paused_validators.contains(&v_bytes) {
            return Err(BridgeError::AlreadyPaused.into());
        }

        // 4. Fail-closed: refuse to pause if it would make the active count
        //    drop below the new effective quorum threshold. We have just
        //    confirmed `validator` is in the current validator set *and* is
        //    not yet paused, so subtracting 1 from the active count is
        //    exact.
        let current_active = self.active_validator_count();
        let new_active = current_active.checked_sub(1).unwrap_or(0);
        let new_threshold = (new_active * 2) / 3 + 1;
        if new_active < new_threshold {
            return Err(BridgeError::PauseWouldBreakQuorum.into());
        }

        // 5. Verify guardian signature over the action-bound payload.
        let payload = concat_prefixed(PAUSE_PAYLOAD_TAG, &v_bytes);
        guardian
            .verify(&payload, signature)
            .map_err(|_| BridgeError::InvalidGuardianSignature)?;

        // 6. Commit: mark the validator paused and return the event.
        self.paused_validators.insert(v_bytes.clone());
        Ok(ValidatorEvent::Paused {
            validator: v_bytes,
            epoch: self.epoch,
        })
    }

    /// Guardian-gated unpause of a single validator.
    ///
    /// The signature is verified against the configured [`Bridge::guardian`]
    /// over the action-bound payload `"BRIDGE_UNPAUSE:" || pk_bytes`, which
    /// is the dual of the pause payload so signatures cannot be replayed
    /// across actions.
    pub fn unpause_validator(
        &mut self,
        validator: &PublicKey,
        signature: &Signature,
    ) -> Result<ValidatorEvent> {
        let guardian = self.guardian.ok_or(BridgeError::NoGuardianConfigured)?;

        let v_bytes = validator.to_bytes().to_vec();

        if !self.validators.contains_pk(validator) {
            return Err(BridgeError::UnknownValidator.into());
        }
        if !self.paused_validators.contains(&v_bytes) {
            return Err(BridgeError::NotPaused.into());
        }

        let payload = concat_prefixed(UNPAUSE_PAYLOAD_TAG, &v_bytes);
        guardian
            .verify(&payload, signature)
            .map_err(|_| BridgeError::InvalidGuardianSignature)?;

        self.paused_validators.remove(&v_bytes);
        Ok(ValidatorEvent::Unpaused {
            validator: v_bytes,
            epoch: self.epoch,
        })
    }

    /// Rejects an inbound message epoch that belongs to a retired validator set.
    ///
    /// An epoch lower than [`Bridge::epoch`] fails. The current epoch and future
    /// epochs pass this narrow guard, so callers must separately authenticate
    /// the inbound message and enforce equality if their policy requires it.
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

    /// Reconfigure the per-window outbound value cap and (re)start the
    /// outbound window fresh at `current_time`.
    ///
    /// `max_per_window == 0` is a valid, intentional configuration meaning
    /// "no outbound" (fail-closed) — use a positive value to actually permit
    /// outbound transfers. `window_size` must be greater than zero.
    pub fn set_outbound_cap(&mut self, max_per_window: i128, window_size: u64, current_time: u64) -> Result<()> {
        if max_per_window < 0 {
            return Err(anyhow!("max_per_window must be >= 0"));
        }
        if window_size == 0 {
            return Err(BridgeError::InvalidWindowSize.into());
        }

        self.max_outbound_per_window = max_per_window;
        self.outbound_window_size = window_size;
        self.outbound_window_start = current_time;
        self.window_outbound_total = 0;
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

    /// Roll the outbound-value window forward if `current_time` has moved
    /// past the end of the current outbound window. Independent from the
    /// inbound window so outbound activity cannot affect inbound accounting.
    fn roll_outbound_window_if_expired(&mut self, current_time: u64) {
        if current_time < self.outbound_window_start {
            return;
        }

        if let Some(window_end) = self.outbound_window_start.checked_add(self.outbound_window_size) {
            if current_time >= window_end {
                self.outbound_window_start = current_time;
                self.window_outbound_total = 0;
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

    /// Admit an outbound transfer of `amount` against the per-window outbound
    /// value cap, tracked on rolling ledger time. See `admit_inbound` for
    /// the symmetric inbound behaviour: this function fails-closed when the
    /// cap is configured as `0`, rejects negative amounts, rejects overflow
    /// on the running total, and rejects attempts that would exceed the
    /// configured outbound cap.
    pub fn admit_outbound(&mut self, amount: i128, current_time: u64) -> Result<()> {
        if amount < 0 {
            return Err(anyhow!("outbound amount must be >= 0"));
        }

        if self.max_outbound_per_window == 0 {
            return Err(anyhow!(BridgeError::OutboundCapExceeded));
        }

        self.roll_outbound_window_if_expired(current_time);

        let new_total = self
            .window_outbound_total
            .checked_add(amount)
            .ok_or_else(|| anyhow!("outbound window total overflow"))?;

        if new_total > self.max_outbound_per_window {
            return Err(BridgeError::OutboundCapExceeded.into());
        }

        self.window_outbound_total = new_total;
        Ok(())
    }
}

/// Helper: build a payload of the form `prefix || suffix` without an
/// intermediate allocation beyond the result vector.
fn concat_prefixed(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(suffix);
    out
}

/// Lowercase hex encoder for the `Display` impl of `ValidatorEvent`. Inlined
/// here (rather than pulling in the `hex` crate as a runtime dependency)
/// because event formatting is the only consumer and the format is trivial.
fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod rotation_test;

#[cfg(test)]
mod rotation_doc_test;

#[cfg(test)]
mod domain_separation_test;

#[cfg(test)]
mod quorum_proof_bound_test;

#[cfg(test)]
mod inbound_cap_test;

#[cfg(test)]
mod window_rollover_test;

#[cfg(test)]
mod validator_bounds_test;

#[cfg(test)]
mod epoch_monotonicity_proptest;

#[cfg(test)]
mod window_guard_test;

#[cfg(test)]
mod window_tuning_doc_test;

#[cfg(test)]
mod outbound_cap_test;

#[cfg(test)]
mod validatorset_proptest;

#[cfg(test)]
mod validator_pause_test;

#[cfg(test)]
mod rotation_churn_test;

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
        let payload = Bridge::quorum_proof_payload(&bridge.bridge_id, &new_set, epoch).unwrap();

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
        let payload = Bridge::quorum_proof_payload(&bridge.bridge_id, &new_set, epoch).unwrap();

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
        let payload = Bridge::quorum_proof_payload(&bridge.bridge_id, &new_set, epoch).unwrap();

        let mut proofs = vec![];
        for i in 0..2 {
            let sig = kp_a[i].sign(&payload);
            proofs.push((kp_a[i].public, sig));
        }

        assert!(bridge.rotate_validators(new_set, epoch, proofs).is_err());
    }
}
