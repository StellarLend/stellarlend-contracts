use alloc::string::{String, ToString };
use soroban_sdk::{Env, Address, Symbol, BytesN, Bytes, IntoVal, Vec, symbol_short};


/// Trait for cross-protocol adapters
pub trait ProtocolAdapter {
    fn protocol_name(&self) -> &'static str;
    fn interact(
        &self,
        env: &Env,
        contract_id: &BytesN<32>,
        function: &Symbol,
        args: Vec<Bytes>
    ) -> Result<Bytes, String>;
}

/// Generic struct for a protocol adapter
pub struct GenericProtocolAdapter;

impl ProtocolAdapter for GenericProtocolAdapter {
    fn protocol_name(&self) -> &'static str {
        "GenericProtocol"
    }
    fn interact(&self, env: &Env, contract_id: &BytesN<32>, function: &Symbol, args: Vec<Bytes> ) -> Result<Bytes, String> {
        // Convert contract_id to Address
        let contract_address = Address::from_string_bytes(contract_id.clone().into());
        // Convert Vec<Bytes> to Vec<Val>
        let mut vals = Vec::new(env);
        for b in args.iter() {
            vals.push_back(b.into_val(env));
        }

        // Call invoke_contract with correct types
        let res = env.invoke_contract::<Bytes> ( &contract_address, &function.clone(), vals );
        match res {
            Ok(val) => Ok(val),
            Err(e) => Err(format!("Cross-protocol call failed: {:?}", e)),
        }
    }
}

/// Enum-based registry for known adapters (static dispatch, Soroban-friendly)
pub enum KnownAdapters {
    Generic(GenericProtocolAdapter),
    // Add more adapters here as neededget
}

impl KnownAdapters {
    pub fn get(protocol: &Symbol) -> Option<Self> {
        match protocol.to_string().as_str() {
            "generic" => Some(KnownAdapters::Generic(GenericProtocolAdapter)),
            // Add more matches here for other protocols
            _ => None,
        }
    }
    pub fn interact(
        &self,
        env: &Env,
        contract_id: &BytesN<32>,
        function: &Symbol,
        args: Vec<Bytes>,
    ) -> Result<Bytes, String> {
        match self {
            KnownAdapters::Generic(adapter) => adapter.interact(env, contract_id, function, args),
            // Add more as needed
        }
    }
}