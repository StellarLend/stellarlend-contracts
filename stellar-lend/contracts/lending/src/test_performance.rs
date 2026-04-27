//! # Performance Benchmark Tests with Thresholds
//!
//! Comprehensive performance benchmarks for critical lending paths with defined thresholds.
//! These tests capture baseline performance metrics using Soroban's Budget API to measure
//! CPU instructions and memory usage.
//!
//! ## Methodology
//!
//! 1. **Threshold Determination**: Thresholds are set based on typical Soroban contract costs:
//!    - Simple operations (views): ~100,000 - 500,000 CPU instructions
//!    - Standard operations (deposit, borrow): ~500,000 - 2,000,000 CPU instructions
//!    - Complex operations (liquidation, flash loan): ~1,000,000 - 5,000,000 CPU instructions
//!    - Memory usage scales with storage operations and contract calls
//!
//! 2. **Test Structure**: Each benchmark:
//!    - Resets the budget before the operation
//!    - Executes the operation
//!    - Captures CPU and memory metrics
//!    - Asserts against thresholds with margin for test environment variance
//!
//! 3. **Smoke Testing**: These benchmarks serve as performance smoke tests to detect:
//!    - Unexpected cost regressions
//!    - Storage layout inefficiencies
//!    - Unbounded iteration or recursion
//!
//! ## Security & Trust Boundaries
//!
//! - **Not a substitute for on-chain metering**: These benchmarks measure test environment costs.
//!   On-chain costs may vary due to ledger state, network conditions, and protocol upgrades.
//! - **Authorization**: All state-changing operations require proper authorization.
//! - **Reentrancy Protection**: Flash loans include reentrancy guards (tested separately).
//! - **Arithmetic Safety**: All operations use checked arithmetic with overflow protection.
//!
//! ## Critical Paths Covered
//!
//! - Deposit (single and multiple)
//! - Borrow with collateral validation
//! - Repay with interest accrual
//! - Withdraw with balance checks
//! - Liquidation (view-only, actual liquidation is stubbed)
//! - Flash loan execution
//! - View functions (health factor, position queries)
//!
//! ## Threshold Reference (Issue #495)
//!
//! | Operation | CPU Threshold | Memory Threshold | Notes |
//! |-----------|--------------|------------------|-------|
//! | Deposit | 2,000,000 | 500,000 | Single deposit operation |
//! | Borrow | 3,000,000 | 750,000 | Includes collateral validation |
//! | Repay | 2,500,000 | 600,000 | With interest calculation |
//! | Withdraw | 2,000,000 | 500,000 | Balance check and transfer |
//! | Liquidation | 1,000,000 | 400,000 | Current stub implementation |
//! | Flash Loan | 5,000,000 | 1,000,000 | Includes callback invocation |
//! | View Queries | 500,000 | 200,000 | Read-only operations |

use crate::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Bytes, Env,
};

// ═══════════════════════════════════════════════════════════════════════
// Performance Threshold Constants
// ═══════════════════════════════════════════════════════════════════════

/// Maximum CPU instructions for deposit operations
const THRESHOLD_DEPOSIT_CPU: u64 = 2_000_000;
/// Maximum memory bytes for deposit operations
const THRESHOLD_DEPOSIT_MEM: u64 = 500_000;

/// Maximum CPU instructions for borrow operations
const THRESHOLD_BORROW_CPU: u64 = 3_000_000;
/// Maximum memory bytes for borrow operations
const THRESHOLD_BORROW_MEM: u64 = 750_000;

/// Maximum CPU instructions for repay operations
const THRESHOLD_REPAY_CPU: u64 = 2_500_000;
/// Maximum memory bytes for repay operations
const THRESHOLD_REPAY_MEM: u64 = 600_000;

/// Maximum CPU instructions for withdraw operations
const THRESHOLD_WITHDRAW_CPU: u64 = 2_000_000;
/// Maximum memory bytes for withdraw operations
const THRESHOLD_WITHDRAW_MEM: u64 = 500_000;

/// Maximum CPU instructions for liquidation operations
const THRESHOLD_LIQUIDATION_CPU: u64 = 1_000_000;
/// Maximum memory bytes for liquidation operations
const THRESHOLD_LIQUIDATION_MEM: u64 = 400_000;

/// Maximum CPU instructions for flash loan operations
const THRESHOLD_FLASH_LOAN_CPU: u64 = 5_000_000;
/// Maximum memory bytes for flash loan operations
const THRESHOLD_FLASH_LOAN_MEM: u64 = 1_000_000;

/// Maximum CPU instructions for view operations
const THRESHOLD_VIEW_CPU: u64 = 500_000;
/// Maximum memory bytes for view operations
const THRESHOLD_VIEW_MEM: u64 = 200_000;

// ═══════════════════════════════════════════════════════════════════════
// Test Setup Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Standard test environment setup with initialized contract
fn setup_test_env() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Initialize with generous limits
    client.initialize(&admin, &1_000_000_000_000i128, &1000);
    client.initialize_deposit_settings(&1_000_000_000_000i128, &100);
    client.initialize_borrow_settings(&1_000_000_000_000i128, &1000);
    client.initialize_withdraw_settings(&100);

    (env, client, admin, asset, collateral_asset)
}

/// Performance measurement helper
fn measure_performance<F>(env: &Env, operation: F) -> (u64, u64)
where
    F: FnOnce(),
{
    env.budget().reset_unlimited();
    let cpu_before = env.budget().cpu_instruction_cost();
    let mem_before = env.budget().memory_bytes_cost();

    operation();

    let cpu_after = env.budget().cpu_instruction_cost();
    let mem_after = env.budget().memory_bytes_cost();

    (cpu_after - cpu_before, mem_after - mem_before)
}

// ═══════════════════════════════════════════════════════════════════════
// Performance Benchmark Tests
// ═══════════════════════════════════════════════════════════════════════

/// Benchmark: Deposit operation performance
///
/// Tests the core deposit flow including:
/// - Authorization validation
/// - Pause state check
/// - Amount validation
/// - Storage write
/// - Event emission
#[test]
fn benchmark_deposit_performance() {
    let (env, client, _admin, asset, _collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    let (cpu, mem) = measure_performance(&env, || {
        client.deposit(&user, &asset, &10_000);
    });

    assert!(
        cpu <= THRESHOLD_DEPOSIT_CPU,
        "Deposit CPU usage {} exceeds threshold {}",
        cpu,
        THRESHOLD_DEPOSIT_CPU
    );
    assert!(
        mem <= THRESHOLD_DEPOSIT_MEM,
        "Deposit memory usage {} exceeds threshold {}",
        mem,
        THRESHOLD_DEPOSIT_MEM
    );
}

/// Benchmark: Multiple sequential deposits
///
/// Verifies that repeated operations maintain consistent performance
/// without unbounded storage cost growth.
#[test]
fn benchmark_multiple_deposits_performance() {
    let (env, client, _admin, asset, _collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // First deposit
    let (cpu1, _mem1) = measure_performance(&env, || {
        client.deposit(&user, &asset, &10_000);
    });

    // Second deposit (accumulating)
    let (cpu2, _mem2) = measure_performance(&env, || {
        client.deposit(&user, &asset, &5_000);
    });

    // Second operation should be comparable or less (same storage slot)
    assert!(
        cpu2 <= cpu1 * 2,
        "Second deposit CPU {} unexpectedly higher than first {}",
        cpu2,
        cpu1
    );
}

/// Benchmark: Borrow operation performance
///
/// Tests the borrow flow including:
/// - Collateral ratio validation (150% minimum)
/// - Debt ceiling check
/// - Interest accrual calculation
/// - Position storage updates
/// - Total debt tracking
#[test]
fn benchmark_borrow_performance() {
    let (env, client, _admin, asset, collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    let (cpu, mem) = measure_performance(&env, || {
        // Borrow with 200% collateral ratio
        client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    });

    assert!(
        cpu <= THRESHOLD_BORROW_CPU,
        "Borrow CPU usage {} exceeds threshold {}",
        cpu,
        THRESHOLD_BORROW_CPU
    );
    assert!(
        mem <= THRESHOLD_BORROW_MEM,
        "Borrow memory usage {} exceeds threshold {}",
        mem,
        THRESHOLD_BORROW_MEM
    );
}

/// Benchmark: Repay operation performance
///
/// Tests the repay flow including:
/// - Debt position lookup
/// - Interest accrual calculation with time elapsed
/// - Partial or full repayment logic
/// - Total debt reduction
#[test]
fn benchmark_repay_performance() {
    let (env, client, _admin, asset, collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // Setup: create a borrow position
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    let (cpu, mem) = measure_performance(&env, || {
        client.repay(&user, &asset, &5_000);
    });

    assert!(
        cpu <= THRESHOLD_REPAY_CPU,
        "Repay CPU usage {} exceeds threshold {}",
        cpu,
        THRESHOLD_REPAY_CPU
    );
    assert!(
        mem <= THRESHOLD_REPAY_MEM,
        "Repay memory usage {} exceeds threshold {}",
        mem,
        THRESHOLD_REPAY_MEM
    );
}

/// Benchmark: Repay with accrued interest
///
/// Tests interest calculation performance when time has elapsed.
#[test]
fn benchmark_repay_with_interest_performance() {
    let (env, client, _admin, asset, collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // Setup: create borrow position
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    // Advance time by 1 year
    env.ledger().with_mut(|li| {
        li.timestamp += 31536000;
    });

    let (cpu, _mem) = measure_performance(&env, || {
        client.repay(&user, &asset, &10_000);
    });

    // Interest calculation adds overhead but should stay within bounds
    assert!(
        cpu <= THRESHOLD_REPAY_CPU * 2, // Allow 2x for interest calc
        "Repay with interest CPU {} exceeds relaxed threshold {}",
        cpu,
        THRESHOLD_REPAY_CPU * 2
    );
}

/// Benchmark: Withdraw operation performance
///
/// Tests the withdraw flow including:
/// - Pause state validation
/// - Balance verification
/// - Amount bounds checking
#[test]
fn benchmark_withdraw_performance() {
    let (env, client, _admin, asset, _collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // Setup: create deposit first
    client.deposit(&user, &asset, &20_000);

    let (cpu, mem) = measure_performance(&env, || {
        let _result = client.withdraw(&user, &asset, &5_000);
    });

    assert!(
        cpu <= THRESHOLD_WITHDRAW_CPU,
        "Withdraw CPU usage {} exceeds threshold {}",
        cpu,
        THRESHOLD_WITHDRAW_CPU
    );
    assert!(
        mem <= THRESHOLD_WITHDRAW_MEM,
        "Withdraw memory usage {} exceeds threshold {}",
        mem,
        THRESHOLD_WITHDRAW_MEM
    );
}

/// Benchmark: Liquidation operation performance
///
/// Tests the liquidation flow. Note: Current implementation is a stub
/// for Issue #391 profiling. Full liquidation will have higher costs.
#[test]
fn benchmark_liquidation_performance() {
    let (env, client, admin, asset, collateral_asset) = setup_test_env();
    let borrower = Address::generate(&env);

    // Setup: create a borrow position for the borrower
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &20_000);

    let (cpu, mem) = measure_performance(&env, || {
        // Note: Current liquidate is a stub implementation
        client.liquidate(&admin, &borrower, &asset, &collateral_asset, &5_000);
    });

    assert!(
        cpu <= THRESHOLD_LIQUIDATION_CPU,
        "Liquidation CPU usage {} exceeds threshold {}",
        cpu,
        THRESHOLD_LIQUIDATION_CPU
    );
    assert!(
        mem <= THRESHOLD_LIQUIDATION_MEM,
        "Liquidation memory usage {} exceeds threshold {}",
        mem,
        THRESHOLD_LIQUIDATION_MEM
    );
}

/// Benchmark: Flash loan operation performance
///
/// Tests the flash loan flow including:
/// - Receiver contract invocation
/// - Token transfers
/// - Fee calculation and collection
/// - Callback validation
#[test]
fn benchmark_flash_loan_performance() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = token::StellarAssetClient::new(&env, &asset);

    // Register receiver using the one from flash_loan_test module
    let receiver_id = env.register(FlashLoanReceiver, ());
    let receiver_address = receiver_id.clone();

    // Initial setup
    client.initialize(&admin, &1_000_000_000_000i128, &1000);
    client.set_flash_loan_fee_bps(&100); // 1% fee

    // Mint assets to the lending contract so it can lend
    token_admin.mint(&contract_id, &100_000);

    // Mint assets to the receiver to cover the fee
    token_admin.mint(&receiver_address, &1000);

    let (cpu, mem) = measure_performance(&env, || {
        client.flash_loan(&receiver_address, &asset, &10_000, &Bytes::new(&env));
    });

    assert!(
        cpu <= THRESHOLD_FLASH_LOAN_CPU,
        "Flash loan CPU usage {} exceeds threshold {}",
        cpu,
        THRESHOLD_FLASH_LOAN_CPU
    );
    assert!(
        mem <= THRESHOLD_FLASH_LOAN_MEM,
        "Flash loan memory usage {} exceeds threshold {}",
        mem,
        THRESHOLD_FLASH_LOAN_MEM
    );
}

/// Benchmark: View function performance (health factor)
///
/// Tests read-only view operations which should be lightweight.
#[test]
fn benchmark_health_factor_view_performance() {
    let (env, client, _admin, asset, collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // Setup: create position
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    let (cpu, mem) = measure_performance(&env, || {
        let _hf = client.get_health_factor(&user);
    });

    assert!(
        cpu <= THRESHOLD_VIEW_CPU,
        "Health factor view CPU {} exceeds threshold {}",
        cpu,
        THRESHOLD_VIEW_CPU
    );
    assert!(
        mem <= THRESHOLD_VIEW_MEM,
        "Health factor view memory {} exceeds threshold {}",
        mem,
        THRESHOLD_VIEW_MEM
    );
}

/// Benchmark: Full position query performance
///
/// Tests the comprehensive position summary view.
#[test]
fn benchmark_user_position_view_performance() {
    let (env, client, _admin, asset, collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // Setup: create both borrow and deposit positions
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    client.deposit(&user, &asset, &5_000);

    let (cpu, mem) = measure_performance(&env, || {
        let _position = client.get_user_position(&user);
    });

    assert!(
        cpu <= THRESHOLD_VIEW_CPU * 2, // Allow 2x for comprehensive query
        "User position view CPU {} exceeds relaxed threshold {}",
        cpu,
        THRESHOLD_VIEW_CPU * 2
    );
    assert!(
        mem <= THRESHOLD_VIEW_MEM * 2,
        "User position view memory {} exceeds relaxed threshold {}",
        mem,
        THRESHOLD_VIEW_MEM * 2
    );
}

/// Benchmark: Deposit operation when paused
///
/// Verifies that paused operations fail fast without expensive computation.
#[test]
fn benchmark_deposit_paused_performance() {
    let (env, client, admin, asset, _collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    // Pause deposits
    client.set_pause(&admin, &PauseType::Deposit, &true);

    let (cpu, _mem) = measure_performance(&env, || {
        let _result = client.try_deposit(&user, &asset, &10_000);
    });

    // Paused operations should be very cheap (just a storage read)
    assert!(
        cpu < THRESHOLD_DEPOSIT_CPU / 2,
        "Paused deposit should be cheap, got {} CPU",
        cpu
    );
}

/// Benchmark: Zero amount validation performance
///
/// Tests that invalid inputs fail fast without expensive computation.
#[test]
fn benchmark_zero_amount_validation_performance() {
    let (env, client, _admin, asset, _collateral_asset) = setup_test_env();
    let user = Address::generate(&env);

    let (cpu, _mem) = measure_performance(&env, || {
        let _result = client.try_deposit(&user, &asset, &0);
    });

    // Validation failures should be very cheap
    assert!(
        cpu < THRESHOLD_DEPOSIT_CPU / 3,
        "Zero amount validation should be cheap, got {} CPU",
        cpu
    );
}

/// Comprehensive performance report
///
/// Runs all critical paths and validates thresholds.
#[test]
fn benchmark_comprehensive_performance_report() {
    let (env, client, _admin, asset, collateral_asset) = setup_test_env();
    let user = Address::generate(&env);
    let _liquidator = Address::generate(&env);

    // Setup for comprehensive testing
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_borrow_settings(&1_000_000_000, &1000);

    // Track all benchmark results
    let mut results: [(&str, u64, u64, u64, u64); 4] = [
        ("", 0, 0, 0, 0),
        ("", 0, 0, 0, 0),
        ("", 0, 0, 0, 0),
        ("", 0, 0, 0, 0),
    ];

    // 1. Deposit
    {
        let (cpu, mem) = measure_performance(&env, || {
            client.deposit(&user, &asset, &10_000);
        });
        results[0] = (
            "Deposit",
            cpu,
            mem,
            THRESHOLD_DEPOSIT_CPU,
            THRESHOLD_DEPOSIT_MEM,
        );
    }

    // 2. Borrow
    {
        let (cpu, mem) = measure_performance(&env, || {
            client.borrow(&user, &asset, &5_000, &collateral_asset, &10_000);
        });
        results[1] = (
            "Borrow",
            cpu,
            mem,
            THRESHOLD_BORROW_CPU,
            THRESHOLD_BORROW_MEM,
        );
    }

    // 3. Repay
    {
        let (cpu, mem) = measure_performance(&env, || {
            client.repay(&user, &asset, &2_000);
        });
        results[2] = ("Repay", cpu, mem, THRESHOLD_REPAY_CPU, THRESHOLD_REPAY_MEM);
    }

    // 4. View operations
    {
        let (cpu, mem) = measure_performance(&env, || {
            let _ = client.get_user_position(&user);
        });
        results[3] = ("View", cpu, mem, THRESHOLD_VIEW_CPU, THRESHOLD_VIEW_MEM);
    }

    // Assertions for CI/CD
    let mut all_passed = true;
    for (name, cpu, _mem, cpu_threshold, _mem_threshold) in results {
        if cpu > cpu_threshold * 2 {
            all_passed = false;
        }
        assert!(
            cpu <= cpu_threshold * 2, // Relaxed for test variance
            "{} CPU {} exceeds relaxed threshold {}",
            name,
            cpu,
            cpu_threshold * 2
        );
    }

    // Final assertion to ensure all tests passed
    assert!(
        all_passed,
        "Some performance benchmarks exceeded thresholds"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Flash Loan Receiver Contract (for testing)
// ═══════════════════════════════════════════════════════════════════════

#[contract]
pub struct FlashLoanReceiver;

#[contractimpl]
impl FlashLoanReceiver {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) -> bool {
        // Transfer principal + fee back to the lender
        let total_repayment = amount + fee;
        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(
            &env.current_contract_address(),
            &initiator,
            &total_repayment,
        );
        true
    }
}
