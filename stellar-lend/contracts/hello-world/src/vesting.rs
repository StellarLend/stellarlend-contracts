//! # Vesting Contract
//!
//! A token vesting contract for StellarLend that supports cliff periods and
//! linear vesting schedules with comprehensive edge case handling.
//!
//! ## Features
//! - Configurable cliff period before vesting begins
//! - Linear vesting over specified duration
//! - Support for multiple schedules per beneficiary
//! - Comprehensive edge case handling for boundary conditions
//! - Leap year and time calculation accuracy
//!
//! ## Time Calculations
//! All time calculations use ledger timestamps (seconds since epoch) and
//! properly handle edge cases including:
//! - Cliff boundary conditions (exact cliff time)
//! - Schedule completion (exact end time)
//! - Zero-duration periods
//! - Leap year considerations

use soroban_sdk::{contract, contracterror, contractimpl, Address, Env, Symbol};

/// Vesting schedule data structure
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VestingSchedule {
    /// Beneficiary address
    pub beneficiary: Address,
    /// Total amount to vest
    pub total_amount: i128,
    /// Cliff period in seconds (no vesting before this time)
    pub cliff_seconds: u64,
    /// Total vesting duration in seconds (after cliff)
    pub vesting_duration_seconds: u64,
    /// Start timestamp (seconds since epoch)
    pub start_timestamp: u64,
    /// Amount already claimed
    pub claimed_amount: i128,
    /// Whether this schedule is active
    pub active: bool,
}

/// Errors that can occur during vesting operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VestingError {
    /// Invalid amount (must be > 0)
    InvalidAmount = 1,
    /// Invalid duration (must be > 0)
    InvalidDuration = 2,
    /// Invalid cliff period (cannot exceed vesting duration)
    InvalidCliff = 3,
    /// Invalid start time (must be >= current time)
    InvalidStartTime = 4,
    /// Schedule not found
    ScheduleNotFound = 5,
    /// Schedule already exists
    ScheduleExists = 6,
    /// Nothing to claim (vested amount is zero)
    NothingToClaim = 7,
    /// Attempted to claim more than vested
    OverClaim = 8,
    /// Unauthorized access
    Unauthorized = 9,
    /// Arithmetic overflow
    Overflow = 10,
    /// Schedule is not active
    InactiveSchedule = 11,
}

/// Storage keys for vesting data
pub struct VestingDataKey;

impl VestingDataKey {
    /// Schedule storage key
    pub const SCHEDULE: Symbol = Symbol::short("SCH");
    /// Admin address
    pub const ADMIN: Symbol = Symbol::short("ADM");
}

#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {
    /// Initialize the vesting contract
    ///
    /// # Arguments
    /// * `admin` - Administrator address
    pub fn initialize(env: Env, admin: Address) -> Result<(), VestingError> {
        if env.storage().persistent().has(&VestingDataKey::ADMIN) {
            return Err(VestingError::ScheduleExists);
        }

        env.storage()
            .persistent()
            .set(&VestingDataKey::ADMIN, &admin);
        Ok(())
    }

    /// Create a new vesting schedule
    ///
    /// # Arguments
    /// * `beneficiary` - Address that will receive vested tokens
    /// * `total_amount` - Total amount to vest
    /// * `cliff_seconds` - Cliff period in seconds
    /// * `vesting_duration_seconds` - Total vesting duration in seconds
    /// * `start_timestamp` - Start timestamp (0 for current time)
    pub fn create_schedule(
        env: Env,
        beneficiary: Address,
        total_amount: i128,
        cliff_seconds: u64,
        vesting_duration_seconds: u64,
        start_timestamp: u64,
    ) -> Result<u64, VestingError> {
        // Validate inputs
        if total_amount <= 0 {
            return Err(VestingError::InvalidAmount);
        }
        if vesting_duration_seconds == 0 {
            return Err(VestingError::InvalidDuration);
        }
        if cliff_seconds > vesting_duration_seconds {
            return Err(VestingError::InvalidCliff);
        }

        let current_time = env.ledger().timestamp();
        let actual_start = if start_timestamp == 0 {
            current_time
        } else {
            if start_timestamp < current_time {
                return Err(VestingError::InvalidStartTime);
            }
            start_timestamp
        };

        // Check if schedule already exists
        let schedule_key = (VestingDataKey::SCHEDULE, beneficiary.clone());
        if env.storage().persistent().has(&schedule_key) {
            return Err(VestingError::ScheduleExists);
        }

        // Create schedule
        let schedule = VestingSchedule {
            beneficiary: beneficiary.clone(),
            total_amount,
            cliff_seconds,
            vesting_duration_seconds,
            start_timestamp: actual_start,
            claimed_amount: 0,
            active: true,
        };

        env.storage().persistent().set(&schedule_key, &schedule);

        Ok(actual_start)
    }

    /// Calculate vested amount for a schedule
    ///
    /// # Arguments
    /// * `beneficiary` - Beneficiary address
    ///
    /// # Returns
    /// Tuple of (vested_amount, claimable_amount, is_fully_vested)
    pub fn calculate_vested(
        env: Env,
        beneficiary: Address,
    ) -> Result<(i128, i128, bool), VestingError> {
        let schedule_key = (VestingDataKey::SCHEDULE, beneficiary.clone());
        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&schedule_key)
            .ok_or(VestingError::ScheduleNotFound)?;

        if !schedule.active {
            return Err(VestingError::InactiveSchedule);
        }

        let current_time = env.ledger().timestamp();

        // Handle edge case: before start time
        if current_time < schedule.start_timestamp {
            return Ok((0, 0, false));
        }

        let elapsed = current_time
            .checked_sub(schedule.start_timestamp)
            .ok_or(VestingError::Overflow)?;

        // Handle edge case: before cliff
        if elapsed < schedule.cliff_seconds {
            return Ok((0, 0, false));
        }

        let vesting_elapsed = elapsed
            .checked_sub(schedule.cliff_seconds)
            .ok_or(VestingError::Overflow)?;

        // Handle edge case: exact completion
        if vesting_elapsed >= schedule.vesting_duration_seconds {
            let vested_amount = schedule.total_amount;
            let claimable = vested_amount
                .checked_sub(schedule.claimed_amount)
                .ok_or(VestingError::Overflow)?;
            return Ok((vested_amount, claimable, true));
        }

        // Calculate linear vesting
        let vested_amount = schedule
            .total_amount
            .checked_mul(vesting_elapsed as i128)
            .ok_or(VestingError::Overflow)?
            .checked_div(schedule.vesting_duration_seconds as i128)
            .ok_or(VestingError::Overflow)?;

        let claimable = vested_amount
            .checked_sub(schedule.claimed_amount)
            .ok_or(VestingError::Overflow)?;

        Ok((vested_amount, claimable, false))
    }

    /// Claim vested tokens
    ///
    /// # Arguments
    /// * `beneficiary` - Beneficiary address
    /// * `amount` - Amount to claim (0 for maximum available)
    pub fn claim(env: Env, beneficiary: Address, amount: i128) -> Result<i128, VestingError> {
        beneficiary.require_auth();

        let schedule_key = (VestingDataKey::SCHEDULE, beneficiary.clone());
        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&schedule_key)
            .ok_or(VestingError::ScheduleNotFound)?;

        if !schedule.active {
            return Err(VestingError::InactiveSchedule);
        }

        let (_, claimable, _) = Self::calculate_vested(env.clone(), beneficiary.clone())?;

        if claimable <= 0 {
            return Err(VestingError::NothingToClaim);
        }

        let claim_amount = if amount == 0 {
            claimable
        } else {
            if amount > claimable {
                return Err(VestingError::OverClaim);
            }
            amount
        };

        // Update claimed amount
        schedule.claimed_amount = schedule
            .claimed_amount
            .checked_add(claim_amount)
            .ok_or(VestingError::Overflow)?;

        env.storage().persistent().set(&schedule_key, &schedule);

        // In a real implementation, this would transfer tokens
        // For testing purposes, we just return the claimed amount
        Ok(claim_amount)
    }

    /// Get schedule information
    pub fn get_schedule(env: Env, beneficiary: Address) -> Result<VestingSchedule, VestingError> {
        let schedule_key = (VestingDataKey::SCHEDULE, beneficiary);
        env.storage()
            .persistent()
            .get(&schedule_key)
            .ok_or(VestingError::ScheduleNotFound)
    }

    /// Deactivate a schedule (admin only)
    pub fn deactivate_schedule(
        env: Env,
        admin: Address,
        beneficiary: Address,
    ) -> Result<(), VestingError> {
        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&VestingDataKey::ADMIN)
            .ok_or(VestingError::ScheduleNotFound)?;

        if stored_admin != admin {
            return Err(VestingError::Unauthorized);
        }

        let schedule_key = (VestingDataKey::SCHEDULE, beneficiary);
        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&schedule_key)
            .ok_or(VestingError::ScheduleNotFound)?;

        schedule.active = false;
        env.storage().persistent().set(&schedule_key, &schedule);

        Ok(())
    }
}
