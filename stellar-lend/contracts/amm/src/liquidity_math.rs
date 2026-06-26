#![no_std]

use crate::math::sqrt;

/// Minimum liquidity permanently locked in the pool on the first deposit.
/// 
/// This constant mitigates the "donation attack" where an attacker deposits
/// a microscopic amount of liquidity, receives 1 LP share, and then donates
/// a massive amount of tokens directly to the pool reserves. Without a minimum
/// locked liquidity, the share price would inflate proportionally, causing
/// subsequent depositors to receive 0 shares due to integer truncation, 
/// allowing the attacker to steal their deposits by withdrawing their 1 share.
///
/// By locking a minimum amount of shares (e.g., 1000) to a dead address (or 
/// simply permanently adding to `total_supply` without a valid owner), the 
/// attacker's cost to inflate the share price to a degree that rounds a victim's
/// deposit to 0 increases by a factor of MINIMUM_LIQUIDITY, making the attack
/// economically unviable.
pub const MINIMUM_LIQUIDITY: i128 = 1000;

/// Calculates the LP shares to mint for a deposit, returning `(shares_to_user, shares_to_lock)`.
///
/// # Arguments
/// * `total_supply` - The current total supply of LP shares.
/// * `amount_0` - The amount of token 0 deposited.
/// * `amount_1` - The amount of token 1 deposited.
/// * `reserve_0` - The pool's reserve of token 0 (before deposit).
/// * `reserve_1` - The pool's reserve of token 1 (before deposit).
pub fn calculate_mint_shares(
    total_supply: i128,
    amount_0: i128,
    amount_1: i128,
    reserve_0: i128,
    reserve_1: i128,
) -> (i128, i128) {
    if total_supply == 0 {
        let product = amount_0.checked_mul(amount_1).expect("overflow in product");
        let liquidity = sqrt(product);
        if liquidity <= MINIMUM_LIQUIDITY {
            panic!("InsufficientLiquidityMinted");
        }
        (liquidity - MINIMUM_LIQUIDITY, MINIMUM_LIQUIDITY)
    } else {
        let liquidity_0 = amount_0.checked_mul(total_supply).expect("overflow in liquidity_0") / reserve_0;
        let liquidity_1 = amount_1.checked_mul(total_supply).expect("overflow in liquidity_1") / reserve_1;
        let liquidity = core::cmp::min(liquidity_0, liquidity_1);
        if liquidity == 0 {
            panic!("InsufficientLiquidityMinted");
        }
        (liquidity, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_deposit() {
        let (shares, lock) = calculate_mint_shares(0, 10000, 10000, 0, 0);
        assert_eq!(shares, 9000);
        assert_eq!(lock, MINIMUM_LIQUIDITY);
    }

    #[test]
    #[should_panic(expected = "InsufficientLiquidityMinted")]
    fn test_first_deposit_insufficient() {
        // sqrt(100 * 10) = 31, which is <= 1000
        calculate_mint_shares(0, 100, 10, 0, 0);
    }

    #[test]
    fn test_subsequent_deposit() {
        // Assuming first deposit was 10000 of each token, total_supply = 10000
        let (shares, lock) = calculate_mint_shares(10000, 2000, 2000, 10000, 10000);
        assert_eq!(shares, 2000);
        assert_eq!(lock, 0);
    }

    #[test]
    fn test_donation_attack_mitigation() {
        // Scenario: 
        // An attacker tries to steal a victim's deposit by inflating the LP share price.
        
        // 1. Attacker makes the initial deposit with minimal amounts.
        // Since MINIMUM_LIQUIDITY is 1000, attacker deposits 1001 of each token.
        let attacker_amount_0 = 1001;
        let attacker_amount_1 = 1001;
        let (attacker_shares, locked_shares) = calculate_mint_shares(0, attacker_amount_0, attacker_amount_1, 0, 0);
        
        assert_eq!(attacker_shares, 1); // sqrt(1001*1001) - 1000 = 1
        assert_eq!(locked_shares, 1000); // permanently locked
        
        let total_supply = attacker_shares + locked_shares; // 1001
        
        // 2. Attacker "donates" a large amount directly to the pool's reserves without minting shares.
        // This artificially inflates the value of a single share.
        let donation = 1_000_000;
        let reserve_0 = attacker_amount_0 + donation;
        let reserve_1 = attacker_amount_1 + donation;
        
        // 3. Victim deposits a large amount of liquidity (e.g., 500,000 of each token).
        let victim_amount_0 = 500_000;
        let victim_amount_1 = 500_000;
        
        let (victim_shares, new_locked) = calculate_mint_shares(
            total_supply, victim_amount_0, victim_amount_1, reserve_0, reserve_1
        );
        
        // Because of the locked minimum liquidity (total_supply = 1001 instead of 1),
        // the victim still receives a proportional amount of shares.
        // shares = min(500,000 * 1001 / 1,001,001, ...) = 500
        assert_eq!(victim_shares, 500);
        assert_eq!(new_locked, 0);
        
        // If MINIMUM_LIQUIDITY was 0, total_supply would be 1, reserve_0 would be 1_000_001.
        // victim_shares would be 500_000 * 1 / 1_000_001 = 0.
        // The attacker would then own 100% of the pool and steal the victim's 500k.
        // The minimum liquidity effectively prevents this attack by changing the truncation ratio.
    }
}