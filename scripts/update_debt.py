with open('stellar-lend/contracts/lending/src/debt.rs', 'r', encoding='utf-8') as f:
    d = f.read()

# Replace compute_new_index panics
old_cni = """    let step1 = current_index
        .checked_mul(rate_bps)
        .expect("BorrowIndex: overflow in rate multiplication");

    let step2 = step1
        .checked_mul(elapsed as i128)
        .expect("BorrowIndex: overflow in elapsed multiplication");

    let denominator = secs_per_year
        .checked_mul(bps_denom)
        .expect("BorrowIndex: denominator overflow");

    let delta = step2
        .checked_div(denominator)
        .expect("BorrowIndex: division by zero in accrual");

    let new_index = current_index
        .checked_add(delta)
        .expect("BorrowIndex: overflow on add");"""

new_cni = """    let delta = (|| -> Option<i128> {
        let step1 = current_index.checked_mul(rate_bps)?;
        let step2 = step1.checked_mul(elapsed as i128)?;
        let denominator = secs_per_year.checked_mul(bps_denom)?;
        step2.checked_div(denominator)
    })().unwrap_or(0);

    let new_index = current_index.checked_add(delta).unwrap_or(current_index);"""

d = d.replace('panic!("BorrowIndex: overflow guard triggered");', 'return current_index;')
d = d.replace(old_cni, new_cni)

# Replace compute_debt
idx = d.find('fn compute_debt(')
if idx != -1:
    end_fn = d.find('\n}\n', idx)
    # inside compute_debt:
    fn_body = d[idx:end_fn]
    old_call = """    position
        .principal
        .checked_mul(current_index)"""
    # find where position.principal starts
    pos_idx = fn_body.find('    position\n        .principal')
    if pos_idx != -1:
        new_fn_body = fn_body[:pos_idx] + """    position
        .principal
        .checked_mul(current_index)
        .and_then(|v| v.checked_div(snapshot))
        .unwrap_or(position.principal)"""
        d = d[:idx] + new_fn_body + d[end_fn:]

# Replace compute_borrow_rate_from_snapshot
old_br = 'try_compute_borrow_rate_from_snapshot(env, snapshot).expect("borrow-rate utilization overflow")'
new_br = 'try_compute_borrow_rate_from_snapshot(env, snapshot).unwrap_or(BorrowRateComputation { utilization_bps: 0, rate_bps: DEFAULT_APR_BPS })'
d = d.replace(old_br, new_br)

with open('stellar-lend/contracts/lending/src/debt.rs', 'w', encoding='utf-8') as f:
    f.write(d)

print("Updated debt.rs successfully")
