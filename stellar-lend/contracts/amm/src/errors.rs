use soroban_sdk::contracterror;

/// Errors that can occur during AMM operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmError {
    /// Invalid swap amount
    InvalidAmount = 1,
    /// Invalid token address
    InvalidToken = 2,
    /// Insufficient liquidity in pool
    InsufficientLiquidity = 3,
    /// Slippage tolerance exceeded
    SlippageExceeded = 4,
    /// Unsupported AMM protocol
    UnsupportedProtocol = 5,
    /// Invalid callback data
    InvalidCallback = 6,
    /// Unauthorized callback caller
    UnauthorizedCaller = 7,
    /// Pool does not exist
    PoolNotFound = 8,
    /// Insufficient balance
    InsufficientBalance = 9,
    /// Operation paused
    OperationPaused = 10,
    /// Overflow in calculation
    Overflow = 11,
    /// Deadline exceeded
    DeadlineExceeded = 12,
    /// Invalid pool parameters
    InvalidPoolParams = 13,
    /// Reentrancy detected
    Reentrancy = 14,
    /// Hook execution failed
    HookFailed = 15,
}