use alloc::string::String;
use soroban_sdk::{Env, Address, Symbol, BytesN, Bytes, IntoVal, Map, Vec, symbol_short};

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

pub struct GenericProtocolAdapter;

impl ProtocolAdapter for GenericProtocolAdapter {
    fn protocol_name(&self) -> &'static str {
        "GenericProtocol"
    }
    fn interact(
        &self,
        env: &Env,
        contract_id: &BytesN<32>,
        function: &Symbol,
        args: Vec<Bytes>
    ) -> Result<Bytes, String> {
        let contract_address = Address::Contract(contract_id.clone());
        let mut vals = Vec::new(env);
        for b in args.iter() {
            vals.push_back(b.into_val(env));
        }
        let res = env.invoke_contract::<Bytes>(&contract_address, function.clone(), vals);
        match res {
            Ok(val) => Ok(val),
            Err(e) => Err(format!("Cross-protocol call failed: {:?}", e)),
        }
    }
}

// Enum wrapper for adapters
#[derive(Clone)]
pub enum RegisteredAdapter {
    Generic(GenericProtocolAdapter),
    // Add more variants for other adapters here as needed
}

impl ProtocolAdapter for RegisteredAdapter {
    fn protocol_name(&self) -> &'static str {
        match self {
            RegisteredAdapter::Generic(adapter) => adapter.protocol_name(),
        }
    }
    fn interact(
        &self,
        env: &Env,
        contract_id: &BytesN<32>,
        function: &Symbol,
        args: Vec<Bytes>
    ) -> Result<Bytes, String> {
        match self {
            RegisteredAdapter::Generic(adapter) => adapter.interact(env, contract_id, function, args),
        }
    }
}

pub struct ProtocolAdapterRegistry {
    adapters: Map<Symbol, RegisteredAdapter>,
}

impl ProtocolAdapterRegistry {
    pub fn new() -> Self {
        let mut adapters = Map::new();
        adapters.set(symbol_short!("generic"), RegisteredAdapter::Generic(GenericProtocolAdapter));
        Self { adapters }
    }

    pub fn get_adapter(&self, protocol: &Symbol) -> Option<&RegisteredAdapter> {
        self.adapters.get(protocol)
    }

    pub fn register_adapter(&mut self, protocol: Symbol, adapter: RegisteredAdapter) {
        self.adapters.set(protocol, adapter);
    }
}