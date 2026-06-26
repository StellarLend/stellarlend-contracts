#![no_std]

/// Calculates the integer square root of `y` using Newton's method.
/// This implementation ensures fast convergence and avoids overflow
/// by carefully choosing the initial guess and avoiding additions
/// that could exceed `i128::MAX`.
pub fn sqrt(y: i128) -> i128 {
    if y < 0 {
        panic!("negative sqrt");
    }
    if y > 3 {
        let mut z = y;
        let mut x = y / 2 + 1;
        while x < z {
            z = x;
            x = (y / x + x) / 2;
        }
        z
    } else if y != 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqrt() {
        assert_eq!(sqrt(0), 0);
        assert_eq!(sqrt(1), 1);
        assert_eq!(sqrt(2), 1);
        assert_eq!(sqrt(3), 1);
        assert_eq!(sqrt(4), 2);
        assert_eq!(sqrt(9), 3);
        assert_eq!(sqrt(16), 4);
        assert_eq!(sqrt(25), 5);
        assert_eq!(sqrt(100), 10);
        assert_eq!(sqrt(1000000), 1000);
        // max square root check for i128
        assert_eq!(sqrt(i128::MAX), 13043817825332782212);
    }

    #[test]
    #[should_panic(expected = "negative sqrt")]
    fn test_sqrt_negative() {
        sqrt(-1);
    }
}