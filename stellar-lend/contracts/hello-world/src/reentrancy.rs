//! Reentrancy guard for the StellarLend hello-world contract.
//!
//! # Design
//!
//! Soroban contracts are single-threaded and each top-level invocation runs in
//! its own ledger snapshot, so classic storage-slot-based reentrancy is the
//! appropriate protection mechanism.  We use **temporary storage** so the lock
//! is never accidentally left set across ledger closes.
//!
//! The public API mirrors the pattern already used by
//! `flash_loan::require_no_active_flash_loan` but is more general:
//!
//! * [`acquire`] – set the lock; panics if already locked.
//! * [`release`] – clear the lock.
//! * [`with_guard`] – RAII-style helper that acquires, runs a closure,
//!   releases, and returns the result.
//! * [`is_locked`] – read-only query used by tests and entrypoints that want
//!   to *check without blocking*.
//!
//! # Storage key
//!
//! The lock lives under `ReentrantLock` in **temporary** storage.  Temporary
//! entries expire with the ledger, so a crash mid-invocation can never leave
//! the contract permanently locked.
//!
//! # Usage in entrypoints
//!
//! Any state-mutating entrypoint that must be protected against re-entrancy
//! (via `invoke_contract` callbacks or cross-contract calls) should call
//! `reentrancy::with_guard(&env, || { … })` around its body, or call
//! `acquire` / `release` explicitly.

use soroban_sdk::{contracterror, contracttype, Env, Symbol};

// ---------------------------------------------------------------------------
// Storage key
// ---------------------------------------------------------------------------

/// Storage key used to hold the reentrancy lock flag.
///
/// Stored in **temporary** storage so it automatically expires at ledger close
/// and cannot permanently brick the contract if a mid-invocation panic occurs.
#[contracttype]
pub enum ReentrantLockKey {
    Lock,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can be returned by the reentrancy guard.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ReentrancyError {
    /// A reentrant call was detected — the lock was already held when
    /// `acquire` was called.
    ReentrantCall = 1,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Storage key symbol used for the temporary lock entry.
///
/// Using a `Symbol` directly (rather than `ReentrantLockKey`) lets us read the
/// flag without allocating a `contracttype` value, which is slightly cheaper
/// in read-only paths.
fn lock_key(env: &Env) -> Symbol {
    Symbol::new(env, "reent_l")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns `true` when the reentrancy lock is currently held.
///
/// This is a read-only check; it does **not** acquire the lock.
pub fn is_locked(env: &Env) -> bool {
    env.storage()
        .temporary()
        .get::<Symbol, bool>(&lock_key(env))
        .unwrap_or(false)
}

/// Acquire the reentrancy lock.
///
/// # Errors
/// Returns [`ReentrancyError::ReentrantCall`] if the lock is already held,
/// indicating a reentrant invocation attempt.
pub fn acquire(env: &Env) -> Result<(), ReentrancyError> {
    if is_locked(env) {
        return Err(ReentrancyError::ReentrantCall);
    }
    env.storage()
        .temporary()
        .set(&lock_key(env), &true);
    Ok(())
}

/// Release the reentrancy lock.
///
/// This is a no-op if the lock is not currently held (idempotent to be safe
/// against double-release in error-recovery paths).
pub fn release(env: &Env) {
    env.storage().temporary().remove(&lock_key(env));
}

/// Execute `f` while holding the reentrancy lock.
///
/// Acquires the lock before calling `f`, releases it after `f` returns
/// (regardless of whether `f` panics — Soroban unwinds storage changes on
/// panic, so the temporary flag is automatically cleared on panic rollback),
/// and propagates the return value.
///
/// # Errors
/// Returns [`ReentrancyError::ReentrantCall`] if the lock is already held
/// before `f` is invoked.
///
/// # Example
/// ```ignore
/// pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, …> {
///     reentrancy::with_guard(&env, || {
///         // … deposit logic …
///     })
/// }
/// ```
pub fn with_guard<T, F>(env: &Env, f: F) -> Result<T, ReentrancyError>
where
    F: FnOnce() -> T,
{
    acquire(env)?;
    let result = f();
    release(env);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{contract, contractimpl, Env};

    // ── Minimal test harness ─────────────────────────────────────────────

    #[contract]
    struct ReentrancyTestContract;

    #[contractimpl]
    impl ReentrancyTestContract {
        pub fn acquire(env: Env) -> Result<(), ReentrancyError> {
            super::acquire(&env)
        }

        pub fn release(env: Env) {
            super::release(&env)
        }

        pub fn is_locked(env: Env) -> bool {
            super::is_locked(&env)
        }

        /// Runs f = acquire, returns whether it succeeded, then does NOT
        /// release — used to simulate a "stuck" lock.
        pub fn acquire_and_hold(env: Env) -> Result<(), ReentrancyError> {
            super::acquire(&env)
        }
    }

    fn setup() -> (Env, ReentrancyTestContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(ReentrancyTestContract, ());
        let client = ReentrancyTestContractClient::new(&env, &id);
        (env, client)
    }

    // ── is_locked ────────────────────────────────────────────────────────

    #[test]
    fn test_is_locked_initially_false() {
        let (_env, client) = setup();
        assert!(!client.is_locked());
    }

    #[test]
    fn test_is_locked_true_after_acquire() {
        let (_env, client) = setup();
        client.acquire().unwrap();
        assert!(client.is_locked());
    }

    #[test]
    fn test_is_locked_false_after_release() {
        let (_env, client) = setup();
        client.acquire().unwrap();
        client.release();
        assert!(!client.is_locked());
    }

    // ── acquire ──────────────────────────────────────────────────────────

    #[test]
    fn test_acquire_succeeds_when_unlocked() {
        let (_env, client) = setup();
        let result = client.try_acquire();
        assert!(result.is_ok(), "acquire should succeed when lock is free");
    }

    #[test]
    fn test_acquire_fails_when_already_locked() {
        let (_env, client) = setup();
        client.acquire().unwrap();
        let result = client.try_acquire();
        assert_eq!(
            result,
            Err(Ok(ReentrancyError::ReentrantCall)),
            "second acquire must return ReentrantCall"
        );
    }

    // ── release ──────────────────────────────────────────────────────────

    #[test]
    fn test_release_when_not_locked_is_noop() {
        let (_env, client) = setup();
        // Must not panic.
        client.release();
        assert!(!client.is_locked());
    }

    #[test]
    fn test_release_allows_reacquire() {
        let (_env, client) = setup();
        client.acquire().unwrap();
        client.release();
        let result = client.try_acquire();
        assert!(
            result.is_ok(),
            "acquire after release should succeed"
        );
    }

    // ── with_guard (tested via the raw env helpers) ──────────────────────

    #[test]
    fn test_with_guard_executes_closure() {
        let env = Env::default();
        let mut ran = false;
        let result: Result<i32, ReentrancyError> = with_guard(&env, || {
            ran = true;
            42
        });
        assert!(ran);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_with_guard_releases_after_closure() {
        let env = Env::default();
        let _ = with_guard(&env, || ());
        assert!(!is_locked(&env), "lock must be released after with_guard");
    }

    #[test]
    fn test_with_guard_returns_err_when_locked() {
        let env = Env::default();
        acquire(&env).unwrap();
        let result: Result<(), ReentrancyError> = with_guard(&env, || ());
        assert_eq!(result, Err(ReentrancyError::ReentrantCall));
        // The lock should still be held (we did not release it above).
        assert!(is_locked(&env));
    }

    // ── error code stability ─────────────────────────────────────────────

    #[test]
    fn test_error_code_stability() {
        assert_eq!(ReentrancyError::ReentrantCall as u32, 1);
    }
}
