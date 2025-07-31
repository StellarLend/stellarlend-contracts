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

/// Protocol adapter registry for managing different protocol integrations
pub struct ProtocolAdapterRegistry;

impl ProtocolAdapterRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn get_adapter(&self, protocol: &Symbol) -> Option<Box<dyn ProtocolAdapter>> {
        match protocol.to_string().as_str() {
            "generic" => Some(Box::new(GenericProtocolAdapter)),
            "uniswap" => Some(Box::new(UniswapAdapter)),
            "aave" => Some(Box::new(AaveAdapter)),
            "compound" => Some(Box::new(CompoundAdapter)),
            _ => None,
        }
    }

    pub fn register_adapter(&self, _protocol: &Symbol, _adapter: Box<dyn ProtocolAdapter>) -> Result<(), String> {
        // In a real implementation, this would store adapters in contract storage
        // For now, we'll use static dispatch
        Ok(())
    }
}

/// Uniswap protocol adapter
pub struct UniswapAdapter;

impl ProtocolAdapter for UniswapAdapter {
    fn protocol_name(&self) -> &'static str {
        "Uniswap"
    }
    
    fn interact(&self, env: &Env, contract_id: &BytesN<32>, function: &Symbol, args: Vec<Bytes>) -> Result<Bytes, String> {
        // Uniswap-specific interaction logic
        let contract_address = Address::from_string_bytes(contract_id.clone().into());
        let mut vals = Vec::new(env);
        for b in args.iter() {
            vals.push_back(b.into_val(env));
        }

        let res = env.invoke_contract::<Bytes>(&contract_address, &function.clone(), vals);
        match res {
            Ok(val) => Ok(val),
            Err(e) => Err(format!("Uniswap call failed: {:?}", e)),
        }
    }
}

/// Aave protocol adapter
pub struct AaveAdapter;

impl ProtocolAdapter for AaveAdapter {
    fn protocol_name(&self) -> &'static str {
        "Aave"
    }
    
    fn interact(&self, env: &Env, contract_id: &BytesN<32>, function: &Symbol, args: Vec<Bytes>) -> Result<Bytes, String> {
        // Aave-specific interaction logic
        let contract_address = Address::from_string_bytes(contract_id.clone().into());
        let mut vals = Vec::new(env);
        for b in args.iter() {
            vals.push_back(b.into_val(env));
        }

        let res = env.invoke_contract::<Bytes>(&contract_address, &function.clone(), vals);
        match res {
            Ok(val) => Ok(val),
            Err(e) => Err(format!("Aave call failed: {:?}", e)),
        }
    }
}

/// Compound protocol adapter
pub struct CompoundAdapter;

impl ProtocolAdapter for CompoundAdapter {
    fn protocol_name(&self) -> &'static str {
        "Compound"
    }
    
    fn interact(&self, env: &Env, contract_id: &BytesN<32>, function: &Symbol, args: Vec<Bytes>) -> Result<Bytes, String> {
        // Compound-specific interaction logic
        let contract_address = Address::from_string_bytes(contract_id.clone().into());
        let mut vals = Vec::new(env);
        for b in args.iter() {
            vals.push_back(b.into_val(env));
        }

        let res = env.invoke_contract::<Bytes>(&contract_address, &function.clone(), vals);
        match res {
            Ok(val) => Ok(val),
            Err(e) => Err(format!("Compound call failed: {:?}", e)),
        }
    }
}

/// Enum-based registry for known adapters (static dispatch, Soroban-friendly)
pub enum KnownAdapters {
    Generic(GenericProtocolAdapter),
    Uniswap(UniswapAdapter),
    Aave(AaveAdapter),
    Compound(CompoundAdapter),
}

impl KnownAdapters {
    pub fn get(protocol: &Symbol) -> Option<Self> {
        match protocol.to_string().as_str() {
            "generic" => Some(KnownAdapters::Generic(GenericProtocolAdapter)),
            "uniswap" => Some(KnownAdapters::Uniswap(UniswapAdapter)),
            "aave" => Some(KnownAdapters::Aave(AaveAdapter)),
            "compound" => Some(KnownAdapters::Compound(CompoundAdapter)),
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
            KnownAdapters::Uniswap(adapter) => adapter.interact(env, contract_id, function, args),
            KnownAdapters::Aave(adapter) => adapter.interact(env, contract_id, function, args),
            KnownAdapters::Compound(adapter) => adapter.interact(env, contract_id, function, args),
        }
    }
}

/// Integration status tracking
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationStatus {
    pub protocol: String,
    pub is_active: bool,
    pub last_interaction: u64,
    pub success_count: u32,
    pub failure_count: u32,
}

impl IntegrationStatus {
    pub fn new(protocol: String) -> Self {
        Self {
            protocol,
            is_active: true,
            last_interaction: 0,
            success_count: 0,
            failure_count: 0,
        }
    }
}