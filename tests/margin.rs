use colada_engine::margin::*;

#[test]
fn margin_pair_vectors() {
    assert!(is_margin_pair_admissible(500, 250, false, 0, 0, 0));
    assert!(!is_margin_pair_admissible(700, 300, false, 0, 0, 0));
    assert!(is_margin_pair_admissible(200, 100, true, 2, 100, 1));
    assert!(!is_margin_pair_admissible(300, 150, true, 2, 100, 1));
    assert!(margin_pair_admissible(500, 250, 10, 10_000));
    assert!(margin_pair_admissible(200, 100, 2, 100));
    assert!(!margin_pair_admissible(200, 100, 3, 100));
    assert!(!margin_pair_admissible(300, 150, 2, 100));
}

#[test]
fn requirements_and_policy_boundaries() {
    for (notional, rate, expected) in [
        (0, u16::MAX, Ok(0)),
        (1, 1, Ok(1)),
        (10_000, 500, Ok(500)),
        (10_001, 500, Ok(501)),
        (u64::MAX, 10_000, Ok(u64::MAX)),
        (u64::MAX, u16::MAX, Err(MarginError::Overflow)),
    ] {
        assert_eq!(initial_margin_requirement(notional, rate), expected);
        assert_eq!(margin_requirement_ceil(notional, rate), expected);
    }
    for (requested, market, accepted) in [
        (0, 0, true),
        (499, 500, false),
        (5_000, 500, true),
        (5_001, 500, false),
        (5_001, 5_001, true),
        (u16::MAX, u16::MAX, true),
        (u16::MAX, 500, false),
    ] {
        assert_eq!(
            validate_requested_initial_margin(requested, market),
            if accepted {
                Ok(())
            } else {
                Err(MarginError::InitialMarginOutOfRange)
            }
        );
    }
}
