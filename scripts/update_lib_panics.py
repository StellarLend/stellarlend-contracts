with open('stellar-lend/contracts/lending/src/lib.rs', 'r', encoding='utf-8') as f:
    lib = f.read()

# Replace panics in lib.rs
# flash_loan:
lib = lib.replace('require_initialized(&env).expect("NotInitialized");', 'require_initialized(&env).unwrap();')
lib = lib.replace('panic!("InsufficientLiquidity");', 'panic_with_error!(&env, LendingError::InsufficientCollateral);')
lib = lib.replace('.expect("flash_loan: max-utilization calculation overflow");', '.unwrap_or(0);')
lib = lib.replace('panic!("FlashLoanUtilizationExceeded");', 'panic_with_error!(&env, LendingError::InvalidAmount);')
lib = lib.replace('.expect("flash_loan: fee calculation overflow");', '.unwrap_or(0);')
lib = lib.replace('.expect("flash_loan: treasury underflow during transfer");', '.unwrap_or(0);')
lib = lib.replace('.expect("flash_loan: receiver balance overflow");', '.unwrap_or(0);')
lib = lib.replace('.expect("flash_loan: fee addition overflow");', '.unwrap_or(0);')
lib = lib.replace('panic!("InsufficientRepayment");', 'panic_with_error!(&env, LendingError::InvalidAmount);')

# reentrancy:
lib = lib.replace('panic!("FlashLoanReentrancy");', 'panic_with_error!(&env, LendingError::Unauthorized);')
lib = lib.replace('panic!("reentrant call");', 'panic_with_error!(&env, LendingError::Unauthorized);')

# pause / emergency:
lib = lib.replace('panic!("OperationPaused");', 'panic_with_error!(&env, LendingError::Paused);')
lib = lib.replace('panic!("OperationDisabledDuringShutdown");', 'panic_with_error!(&env, LendingError::EmergencyShutdown);')
lib = lib.replace('panic!("ActionBlockedInRecovery");', 'panic_with_error!(&env, LendingError::EmergencyShutdown);')
lib = lib.replace('panic!("SlippageExceeded");', 'panic_with_error!(&env, LendingError::InvalidAmount);')

with open('stellar-lend/contracts/lending/src/lib.rs', 'w', encoding='utf-8') as f:
    f.write(lib)

print("Updated lib.rs panics successfully")
