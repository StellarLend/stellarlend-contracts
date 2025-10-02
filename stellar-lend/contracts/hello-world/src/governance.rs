#![allow(dead_code)]
use soroban_sdk::{contracttype, Address, Env, Map, Symbol, Vec};

/// Unified proposal structure for both governance and MultiSig operations
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: soroban_sdk::String,
    pub created: u64,
    pub voting_ends: u64,
    pub queued_until: u64,
    pub for_votes: i128,
    pub against_votes: i128,
    pub executed: bool,
    pub action: ProposalAction,
    pub proposal_type: ProposalType,
}

/// Vote receipt for tracking individual votes
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct VoteReceipt {
    pub voter: Address,
    pub support: bool,
    pub weight: i128,
}

/// Types of proposals in the unified system
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum ProposalType {
    Governance,  // Regular governance proposal
    MultiSig,    // MultiSig admin operation
}

/// Specific actions that can be proposed
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum ProposalAction {
    SetMinCollateralRatio(i128),
    SetFlashLoanFeeBps(i128),
    SetOracle(Address),
    SetRiskParams(i128, i128), // close_factor, liquidation_incentive
    SetPauseSwitches(bool, bool, bool, bool), // pause_borrow, pause_deposit, pause_withdraw, pause_liquidate
    SetEmergencyManager(Address, bool),
    SetAdmin(Address),
    SetMultiSigThreshold(i128),
    SetMultiSigSigners(Vec<Address>),
}

/// MultiSig configuration
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct MultiSigConfig {
    pub signers: Vec<Address>,
    pub threshold: i128,
    pub timelock_delay: u64,
}

/// MultiSig signature for approval tracking
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct MultiSigSignature {
    pub signer: Address,
    pub proposal_id: u64,
    pub signed_at: u64,
}

/// Storage layer for governance and MultiSig data
pub struct GovStorage;

impl GovStorage {
    fn proposals_key(env: &Env) -> Symbol { Symbol::new(env, "gov_proposals") }
    fn receipts_key(env: &Env) -> Symbol { Symbol::new(env, "gov_receipts") }
    fn counter_key(env: &Env) -> Symbol { Symbol::new(env, "gov_counter") }
    fn quorum_bps_key(env: &Env) -> Symbol { Symbol::new(env, "gov_quorum_bps") }
    fn timelock_key(env: &Env) -> Symbol { Symbol::new(env, "gov_timelock") }
    fn delegation_key(env: &Env) -> Symbol { Symbol::new(env, "gov_delegation") }
    fn multisig_config_key(env: &Env) -> Symbol { Symbol::new(env, "multisig_config") }
    fn multisig_signatures_key(env: &Env) -> Symbol { Symbol::new(env, "multisig_sigs") }

    pub fn next_id(env: &Env) -> u64 {
        let id: u64 = env.storage().instance().get(&Self::counter_key(env)).unwrap_or(0);
        env.storage().instance().set(&Self::counter_key(env), &(id + 1));
        id + 1
    }

    pub fn save_proposal(env: &Env, p: &Proposal) {
        let mut map: Map<u64, Proposal> = env.storage().instance().get(&Self::proposals_key(env)).unwrap_or_else(|| Map::new(env));
        map.set(p.id, p.clone());
        env.storage().instance().set(&Self::proposals_key(env), &map);
    }

    pub fn get_proposal(env: &Env, id: u64) -> Option<Proposal> {
        let map: Map<u64, Proposal> = env.storage().instance().get(&Self::proposals_key(env)).unwrap_or_else(|| Map::new(env));
        map.get(id)
    }

    pub fn save_receipt(env: &Env, id: u64, r: &VoteReceipt) {
        let key = (Self::receipts_key(env), id);
        let mut map: Map<Address, VoteReceipt> = env.storage().instance().get(&key).unwrap_or_else(|| Map::new(env));
        map.set(r.voter.clone(), r.clone());
        env.storage().instance().set(&key, &map);
    }

    pub fn get_quorum_bps(env: &Env) -> i128 { env.storage().instance().get(&Self::quorum_bps_key(env)).unwrap_or(1000) }
    pub fn set_quorum_bps(env: &Env, bps: i128) { env.storage().instance().set(&Self::quorum_bps_key(env), &bps); }
    pub fn get_timelock(env: &Env) -> u64 { env.storage().instance().get(&Self::timelock_key(env)).unwrap_or(60) }
    pub fn set_timelock(env: &Env, secs: u64) { env.storage().instance().set(&Self::timelock_key(env), &secs); }

    // MultiSig specific storage methods
    pub fn save_multisig_config(env: &Env, config: &MultiSigConfig) {
        env.storage().instance().set(&Self::multisig_config_key(env), config);
    }

    pub fn get_multisig_config(env: &Env) -> Option<MultiSigConfig> {
        env.storage().instance().get(&Self::multisig_config_key(env))
    }

    pub fn save_multisig_signature(env: &Env, sig: &MultiSigSignature) {
        let key = (Self::multisig_signatures_key(env), sig.proposal_id);
        let mut map: Map<Address, MultiSigSignature> = env.storage().instance().get(&key).unwrap_or_else(|| Map::new(env));
        map.set(sig.signer.clone(), sig.clone());
        env.storage().instance().set(&key, &map);
    }

    pub fn get_multisig_signatures(env: &Env, proposal_id: u64) -> Map<Address, MultiSigSignature> {
        let key = (Self::multisig_signatures_key(env), proposal_id);
        env.storage().instance().get(&key).unwrap_or_else(|| Map::new(env))
    }
}

/// Unified governance and MultiSig implementation
pub struct Governance;

impl Governance {
    /// Create a governance proposal (token-based voting)
    pub fn propose(env: &Env, proposer: &Address, title: soroban_sdk::String, voting_period_secs: u64, action: ProposalAction) -> Proposal {
        let now = env.ledger().timestamp();
        let id = GovStorage::next_id(env);
        let p = Proposal { 
            id, 
            proposer: proposer.clone(), 
            title, 
            created: now, 
            voting_ends: now + voting_period_secs, 
            queued_until: 0, 
            for_votes: 0, 
            against_votes: 0, 
            executed: false,
            action,
            proposal_type: ProposalType::Governance,
        };
        GovStorage::save_proposal(env, &p);
        p
    }

    /// Create a MultiSig proposal (threshold-based signing)
    pub fn propose_multisig(env: &Env, proposer: &Address, title: soroban_sdk::String, action: ProposalAction) -> Proposal {
        let now = env.ledger().timestamp();
        let id = GovStorage::next_id(env);
        let p = Proposal { 
            id, 
            proposer: proposer.clone(), 
            title, 
            created: now, 
            voting_ends: now + 86400, // 24 hours for MultiSig voting
            queued_until: 0, 
            for_votes: 0, 
            against_votes: 0, 
            executed: false,
            action,
            proposal_type: ProposalType::MultiSig,
        };
        GovStorage::save_proposal(env, &p);
        p
    }

    /// Vote on a governance proposal
    pub fn vote(env: &Env, id: u64, voter: &Address, support: bool, weight: i128) -> Proposal {
        let mut p = GovStorage::get_proposal(env, id).unwrap();
        if env.ledger().timestamp() > p.voting_ends { return p; }
        if p.proposal_type != ProposalType::Governance { return p; }
        
        if support { p.for_votes += weight; } else { p.against_votes += weight; }
        GovStorage::save_receipt(env, id, &VoteReceipt { voter: voter.clone(), support, weight });
        GovStorage::save_proposal(env, &p);
        p
    }

    /// Sign a MultiSig proposal
    pub fn sign_multisig(env: &Env, id: u64, signer: &Address) -> Proposal {
        let mut p = GovStorage::get_proposal(env, id).unwrap();
        if env.ledger().timestamp() > p.voting_ends { return p; }
        if p.proposal_type != ProposalType::MultiSig { return p; }

        // Verify signer is authorized
        if let Some(config) = GovStorage::get_multisig_config(env) {
            let mut is_authorized = false;
            for authorized_signer in config.signers.iter() {
                if authorized_signer == *signer {
                    is_authorized = true;
                    break;
                }
            }
            if !is_authorized { return p; }
        } else {
            return p; // No MultiSig config set
        }

        // Record signature
        let sig = MultiSigSignature {
            signer: signer.clone(),
            proposal_id: id,
            signed_at: env.ledger().timestamp(),
        };
        GovStorage::save_multisig_signature(env, &sig);

        // Count signatures to update for_votes
        let signatures = GovStorage::get_multisig_signatures(env, id);
        p.for_votes = signatures.len() as i128;
        GovStorage::save_proposal(env, &p);
        p
    }

    /// Queue a proposal after voting/signing period ends
    pub fn queue(env: &Env, id: u64) -> Proposal {
        let mut p = GovStorage::get_proposal(env, id).unwrap();
        let now = env.ledger().timestamp();
        
        if now < p.voting_ends { return p; }

        let can_queue = match p.proposal_type {
            ProposalType::Governance => {
                let quorum = GovStorage::get_quorum_bps(env);
                let total = p.for_votes + p.against_votes;
                if total == 0 { false } else { (p.for_votes * 10000 / total) >= quorum }
            },
            ProposalType::MultiSig => {
                if let Some(config) = GovStorage::get_multisig_config(env) {
                    p.for_votes >= config.threshold
                } else {
                    false
                }
            }
        };

        if can_queue {
            let timelock_delay = match p.proposal_type {
                ProposalType::Governance => GovStorage::get_timelock(env),
                ProposalType::MultiSig => {
                    GovStorage::get_multisig_config(env)
                        .map(|c| c.timelock_delay)
                        .unwrap_or(GovStorage::get_timelock(env))
                }
            };
            p.queued_until = now + timelock_delay;
        }
        
        GovStorage::save_proposal(env, &p);
        p
    }

    /// Execute a queued proposal
    pub fn execute(env: &Env, id: u64) -> Proposal {
        let mut p = GovStorage::get_proposal(env, id).unwrap();
        let now = env.ledger().timestamp();
        if now >= p.queued_until && p.queued_until != 0 { 
            p.executed = true; 
        }
        GovStorage::save_proposal(env, &p);
        p
    }

    /// Delegate voting power (governance only)
    pub fn delegate(env: &Env, from: &Address, to: &Address) {
        let key = (GovStorage::delegation_key(env), from.clone());
        env.storage().instance().set(&key, to);
    }

    /// Get delegate for address
    pub fn get_delegate(env: &Env, from: &Address) -> Option<Address> {
        let key = (GovStorage::delegation_key(env), from.clone());
        env.storage().instance().get(&key)
    }

    /// Set MultiSig configuration (admin only initially, then governance)
    pub fn set_multisig_config(env: &Env, config: &MultiSigConfig) {
        GovStorage::save_multisig_config(env, config);
    }

    /// Get current MultiSig configuration
    pub fn get_multisig_config(env: &Env) -> Option<MultiSigConfig> {
        GovStorage::get_multisig_config(env)
    }

    /// Check if a proposal meets execution requirements
    pub fn can_execute(env: &Env, id: u64) -> bool {
        if let Some(proposal) = GovStorage::get_proposal(env, id) {
            let now = env.ledger().timestamp();
            !proposal.executed && proposal.queued_until > 0 && now >= proposal.queued_until
        } else {
            false
        }
    }

    /// Get signature count for MultiSig proposal
    pub fn get_signature_count(env: &Env, proposal_id: u64) -> i128 {
        GovStorage::get_multisig_signatures(env, proposal_id).len() as i128
    }
}
