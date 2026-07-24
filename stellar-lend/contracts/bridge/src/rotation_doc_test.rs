use crate::ValidatorSet;

fn validator_set_with_unique_keys(count: usize) -> ValidatorSet {
    ValidatorSet {
        validators: (0..count).map(|index| vec![index as u8; 32]).collect(),
    }
}

#[test]
fn documented_quorum_table_matches_threshold() {
    let documented_cases = [(3, 3), (4, 3), (5, 4), (6, 5), (7, 5), (10, 7), (32, 22)];

    for (validator_count, documented_threshold) in documented_cases {
        let validator_set = validator_set_with_unique_keys(validator_count);
        assert_eq!(
            validator_set.threshold(),
            documented_threshold,
            "ROTATION_PROTOCOL.md quorum mismatch for n={validator_count}",
        );
    }
}

#[test]
fn worked_five_validator_rotation_requires_four_signatures() {
    let current_set = validator_set_with_unique_keys(5);

    assert_eq!(current_set.threshold(), 4);
    assert!(3 < current_set.threshold());
    assert!(4 >= current_set.threshold());
}
