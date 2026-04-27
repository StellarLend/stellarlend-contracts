/// Event parser for decoding smart contract events
use crate::error::{IndexerError, IndexerResult};
use crate::models::CreateEvent;
use ethers::abi::{Abi, Event as AbiEvent, RawLog};
use ethers::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Event parser that decodes blockchain logs into structured events
pub struct EventParser {
    /// Map of contract addresses to their ABIs
    contract_abis: HashMap<String, Arc<Abi>>,

    /// Map of event signatures to event definitions
    event_signatures: HashMap<H256, (String, Arc<AbiEvent>)>,
}

impl EventParser {
    /// Create a new event parser
    pub fn new() -> Self {
        Self {
            contract_abis: HashMap::new(),
            event_signatures: HashMap::new(),
        }
    }

    /// Register a contract ABI for parsing
    ///
    /// # Arguments
    /// * `contract_address` - Contract address
    /// * `abi_json` - ABI as JSON string
    pub fn register_contract(
        &mut self,
        contract_address: &str,
        abi_json: &str,
    ) -> IndexerResult<()> {
        let abi: Abi = serde_json::from_str(abi_json)
            .map_err(|e| IndexerError::EventParsing(format!("Invalid ABI: {}", e)))?;

        let abi_arc = Arc::new(abi.clone());
        self.contract_abis
            .insert(contract_address.to_lowercase(), abi_arc.clone());

        // Index event signatures for fast lookup
        for event in abi.events() {
            let signature = event.signature();
            self.event_signatures.insert(
                signature,
                (contract_address.to_string(), Arc::new(event.clone())),
            );
        }

        Ok(())
    }

    /// Parse a log into a structured event
    ///
    /// # Arguments
    /// * `log` - Ethereum log from blockchain
    ///
    /// # Returns
    /// Parsed event or None if the log is not recognized
    pub fn parse_log(&self, log: &Log) -> IndexerResult<Option<CreateEvent>> {
        // Check if we have an ABI for this contract
        let contract_address = format!("{:?}", log.address).to_lowercase();

        let _abi = match self.contract_abis.get(&contract_address) {
            Some(abi) => abi,
            None => return Ok(None), // Unknown contract, skip
        };

        // Get the event signature (first topic)
        if log.topics.is_empty() {
            return Ok(None);
        }

        let event_signature = log.topics[0];

        // Find the event definition
        let (_, event_def) = match self.event_signatures.get(&event_signature) {
            Some(def) => def,
            None => return Ok(None), // Unknown event, skip
        };

        // Decode the event
        let raw_log = RawLog {
            topics: log.topics.clone(),
            data: log.data.to_vec(),
        };

        let decoded = event_def
            .parse_log(raw_log)
            .map_err(|e| IndexerError::EventParsing(format!("Failed to decode event: {}", e)))?;

        // Convert decoded parameters to JSON
        let mut event_data = serde_json::Map::new();

        for param in decoded.params {
            let value = self.token_to_json(&param.value)?;
            event_data.insert(param.name, value);
        }

        Ok(Some(CreateEvent {
            contract_address,
            event_name: event_def.name.clone(),
            block_number: log
                .block_number
                .ok_or_else(|| IndexerError::EventParsing("Missing block number".to_string()))?
                .as_u64(),
            transaction_hash: format!(
                "{:?}",
                log.transaction_hash
                    .ok_or_else(|| IndexerError::EventParsing(
                        "Missing transaction hash".to_string()
                    ))?
            ),
            log_index: log
                .log_index
                .ok_or_else(|| IndexerError::EventParsing("Missing log index".to_string()))?
                .as_u32(),
            event_data: Value::Object(event_data),
        }))
    }

    /// Convert ABI token to JSON value
    fn token_to_json(&self, token: &ethers::abi::Token) -> IndexerResult<Value> {
        match token {
            ethers::abi::Token::Address(addr) => Ok(Value::String(format!("{:?}", addr))),
            ethers::abi::Token::Uint(val) | ethers::abi::Token::Int(val) => {
                Ok(Value::String(val.to_string()))
            }
            ethers::abi::Token::Bool(b) => Ok(Value::Bool(*b)),
            ethers::abi::Token::String(s) => Ok(Value::String(s.clone())),
            ethers::abi::Token::Bytes(b) | ethers::abi::Token::FixedBytes(b) => {
                Ok(Value::String(format!("0x{}", hex::encode(b))))
            }
            ethers::abi::Token::Array(tokens) | ethers::abi::Token::FixedArray(tokens) => {
                let values: Result<Vec<_>, _> =
                    tokens.iter().map(|t| self.token_to_json(t)).collect();
                Ok(Value::Array(values?))
            }
            ethers::abi::Token::Tuple(tokens) => {
                let values: Result<Vec<_>, _> =
                    tokens.iter().map(|t| self.token_to_json(t)).collect();
                Ok(Value::Array(values?))
            }
        }
    }

    /// Get list of registered contract addresses
    pub fn get_registered_contracts(&self) -> Vec<String> {
        self.contract_abis.keys().cloned().collect()
    }

    /// Check if a contract is registered
    pub fn is_contract_registered(&self, address: &str) -> bool {
        self.contract_abis.contains_key(&address.to_lowercase())
    }
}

impl Default for EventParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard ERC20 Transfer event ABI
pub const ERC20_TRANSFER_ABI: &str = r#"
{
  "anonymous": false,
  "inputs": [
    {"indexed": true, "name": "from", "type": "address"},
    {"indexed": true, "name": "to", "type": "address"},
    {"indexed": false, "name": "value", "type": "uint256"}
  ],
  "name": "Transfer",
  "type": "event"
}
"#;

/// Standard ERC20 Approval event ABI
pub const ERC20_APPROVAL_ABI: &str = r#"
{
  "anonymous": false,
  "inputs": [
    {"indexed": true, "name": "owner", "type": "address"},
    {"indexed": true, "name": "spender", "type": "address"},
    {"indexed": false, "name": "value", "type": "uint256"}
  ],
  "name": "Approval",
  "type": "event"
}
"#;

/// Create a basic ERC20 ABI with Transfer and Approval events
pub fn create_erc20_abi() -> String {
    format!(
        r#"[{}, {}]"#,
        ERC20_TRANSFER_ABI.trim(),
        ERC20_APPROVAL_ABI.trim()
    )
}
