use anyhow::{anyhow, Result};
use bincode;
use ed25519_dalek::{PublicKey, Signature, Verifier};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Store validator public keys as raw bytes so the struct remains serde-friendly
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorSet {
    pub validators: Vec<Vec<u8>>, // each is PublicKey::to_bytes()
}

impl ValidatorSet {
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    pub fn threshold(&self) -> usize {
        // Supermajority: > 2/3 of validators
        let n = self.len();
        (n * 2) / 3 + 1
    }

    pub fn contains_pk(&self, pk: &PublicKey) -> bool {
        let b = pk.to_bytes();
        self.validators.iter().any(|v| v.as_slice() == b.as_ref())
    }

    pub fn to_bytes_vec(&self) -> Vec<Vec<u8>> {
        self.validators.clone()
    }
}

#[derive(Clone, Debug)]
pub struct Bridge {
    pub epoch: u64,
    pub validators: ValidatorSet,
}

impl Bridge {
    pub fn new(initial: ValidatorSet) -> Self {
        Bridge { epoch: 0, validators: initial }
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
}

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
