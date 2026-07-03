pub fn compute_smoothed_rate(
    last_rate: i128,
    target_rate: i128,
    max_step: i128,
    elapsed: u32,
    hysteresis_bps: i128,
) -> i128 {
    let adjusted_target = apply_hysteresis(last_rate, target_rate, hysteresis_bps);
    if elapsed == 0 || max_step == i128::MAX {
        return adjusted_target;
    }
    let max_change = max_step.saturating_mul(elapsed as i128);
    let diff = adjusted_target
        .checked_sub(last_rate)
        .unwrap_or(if adjusted_target >= last_rate {
            i128::MAX
        } else {
            i128::MIN
        });

    if diff > 0 {
        last_rate
            .checked_add(diff.min(max_change))
            .unwrap_or(adjusted_target)
    } else {
        let decrease = diff.checked_abs().unwrap_or(i128::MAX).min(max_change);
        last_rate.checked_sub(decrease).unwrap_or(adjusted_target)
    }
}
