use soroban_sdk::{contracttype, Address, Map, String, Symbol, Vec};

/// Storage keys for AMM-related data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum AmmDataKey {
    /// Admin address
    Admin,
    /// Pool information: Map<(Address, Address, String), PoolInfo>
    PoolInfo(Address, Address, String),
    /// User liquidity positions: Map<(Address, Address, Address, String), i128>
    LiquidityPosition(Address, Address, Address, String), // user, token_a, token_b, protocol
    /// Supported protocols: Vec<String>
    SupportedProtocols,
    /// Protocol configurations: Map<String, ProtocolConfig>
    ProtocolConfig(String),
    /// Callback validations: Map<Address, CallbackData>
    CallbackValidation(Address),
    /// Pause switches: Map<Symbol, bool>
    PauseSwitches,
    /// AMM analytics
    AmmAnalytics,
    /// Hook configurations: Map<Symbol, HookConfig>
    HookConfig(Symbol),
}

/// Pool information structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolInfo {
    /// First token address
    pub token_a: Address,
    /// Second token address
    pub token_b: Address,
    /// Reserve of token A
    pub reserve_a: i128,
    /// Reserve of token B
    pub reserve_b: i128,
    /// Total liquidity tokens
    pub total_liquidity: i128,
    /// Fee rate in basis points (e.g., 300 = 0.3%)
    pub fee_rate: i128,
}

/// AMM protocol configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolConfig {
    /// Protocol name
    pub name: String,
    /// Protocol contract address
    pub contract_address: Address,
    /// Whether protocol is enabled
    pub enabled: bool,
    /// Fee rate for this protocol
    pub fee_rate: i128,
    /// Maximum slippage allowed
    pub max_slippage: i128,
}

/// Callback data for AMM operations
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CallbackData {
    /// Operation type (swap, add_liquidity, remove_liquidity)
    pub operation: Symbol,
    /// User address
    pub user: Address,
    /// Token addresses involved
    pub tokens: Vec<Address>,
    /// Amounts involved
    pub amounts: Vec<i128>,
    /// Additional metadata
    pub metadata: Map<Symbol, String>,
    /// Timestamp
    pub timestamp: u64,
    /// Nonce for replay protection
    pub nonce: u64,
}

/// Hook configuration for lending integration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HookConfig {
    /// Hook name
    pub name: Symbol,
    /// Target contract address
    pub target_contract: Address,
    /// Function to call
    pub function_name: Symbol,
    /// Whether hook is enabled
    pub enabled: bool,
    /// Hook priority (lower = higher priority)
    pub priority: u32,
}

/// Swap parameters
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapParams {
    /// Input token
    pub token_in: Address,
    /// Output token
    pub token_out: Address,
    /// Input amount
    pub amount_in: i128,
    /// Minimum output amount
    pub min_amount_out: i128,
    /// Deadline timestamp
    pub deadline: u64,
    /// AMM protocol to use
    pub protocol: String,
}

/// Liquidity parameters
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidityParams {
    /// First token
    pub token_a: Address,
    /// Second token
    pub token_b: Address,
    /// Amount of token A
    pub amount_a: i128,
    /// Amount of token B
    pub amount_b: i128,
    /// Minimum liquidity tokens
    pub min_liquidity: i128,
    /// Deadline timestamp
    pub deadline: u64,
    /// AMM protocol to use
    pub protocol: String,
}

/// AMM analytics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AmmAnalytics {
    /// Total swap volume
    pub total_swap_volume: i128,
    /// Total liquidity added
    pub total_liquidity_added: i128,
    /// Total liquidity removed
    pub total_liquidity_removed: i128,
    /// Number of swaps
    pub swap_count: u64,
    /// Number of liquidity operations
    pub liquidity_operations: u64,
    /// Total fees collected
    pub total_fees: i128,
}

/// Swap result
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SwapResult {
    /// Amount of output tokens received
    pub amount_out: i128,
    /// Fee paid
    pub fee_paid: i128,
    /// Price impact
    pub price_impact: i128,
    /// Protocol used
    pub protocol_used: String,
}