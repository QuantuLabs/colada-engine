use colada_engine::discovery::*;

#[test]
fn budget_vectors() {
    let full = depth_multiple_bps(2);
    assert_eq!(depth_budget(499_999_999, full).unwrap(), 499_999_999);
    assert_eq!(depth_budget(1_000_000_000, full).unwrap(), 1_000_000_000);
    assert_eq!(
        depth_budget(1_000_000_000, depth_multiple_bps(1)).unwrap(),
        250_000_000
    );
    assert_eq!(depth_multiple_bps(4), full);
    assert_eq!(
        depth_budget(u128::from(u64::MAX), full).unwrap(),
        DEPTH_BUDGET_CAP
    );
    assert!(depth_budget(u128::MAX, full).is_err());
}

#[test]
fn point_cap_vectors() {
    let caps: [(u128, (u128, u128), (u128, u128)); 9] = [
        (0, (0, 0), (0, 0)),
        (145046, (3626150, 5076610), (14504, 36261)),
        (1448615, (36215375, 50701525), (144861, 362153)),
        (10000000, (250000000, 350000000), (1000000, 2500000)),
        (20000000, (500000000, 700000000), (2000000, 5000000)),
        (28571428, (714285700, 999999980), (2857142, 7142857)),
        (28571429, (714285725, 1000000015), (2857142, 7142857)),
        (100000000, (2500000000, 3500000000), (10000000, 25000000)),
        (
            1000000000,
            (25000000000, 35000000000),
            (100000000, 250000000),
        ),
    ];
    for (floor, secure, interim) in caps {
        assert_eq!(point_caps(floor, true), secure);
        assert_eq!(point_caps(floor, false), interim);
    }
}

#[test]
fn support_vectors() {
    let sampled = Some((1000, 1120));
    assert_eq!(overlap(&[(POINT, true), (sampled, true)]).unwrap(), sampled);
    assert_eq!(overlap(&[(POINT, true), (None, false)]).unwrap(), None);
    assert_eq!(overlap(&[(POINT, true), (POINT, true)]).unwrap(), POINT);
    assert_eq!(overlap(&[(POINT, true)]).unwrap(), POINT);
    assert_eq!(
        overlap(&[(Some((995, 1125)), false), (POINT, true)]).unwrap(),
        Some((995, 1125))
    );
    assert_eq!(
        overlap(&[
            overlap(&[(POINT, true), (POINT, true)])
                .map(|s| (s, true))
                .unwrap(),
            (sampled, true)
        ])
        .unwrap(),
        sampled
    );
    assert!(overlap(&[(sampled, true), (Some((1000, 1110)), false)]).is_err());
    for lone in [None, Some((1000, 1050)), Some((u32::MAX, 0))] {
        assert!(overlap(&[(POINT, true), (lone, true)]).is_err());
    }
    assert_eq!(meet(POINT, sampled).map(|(s, e)| e < s), Some(true));
    for (floor, position, shared) in [
        (0u128, 0u128, 0u128),
        (9, 0, 2),
        (10, 1, 2),
        (10_000_000, 1_000_000, 2_500_000),
        (4_000_000_000, 400_000_000, 1_000_000_000),
    ] {
        assert_eq!(sampled_caps(floor), (position, shared));
    }
}

#[test]
fn live_bounds_vectors() {
    let check = |oi_bps, equity, policy, notional| {
        validate_live_notional(oi_bps, equity, 1_000, policy, notional).is_ok()
    };
    assert!(check(2_000, 25_000_000_000, 277_021_880, 277_021_880));
    assert!(!check(2_000, 25_000_000_000, 277_021_880, 277_021_881));
    assert!(check(2_000, 25_000_000_000, u128::MAX, 2_500_000_000));
    assert!(!check(2_000, 25_000_000_000, u128::MAX, 2_500_000_001));
    assert!(!check(500, 25_000_000_000, u128::MAX, 1_250_000_001));
    assert!(!check(2_000, 0, u128::MAX, 1));
}

#[test]
fn individual_live_bounds() {
    let hard = u128::from(1_u64 << 44);
    let live = |oi_bps, equity, kappa, policy, notional: u64| {
        validate_live_notional(oi_bps, equity, kappa, policy, notional).is_ok()
    };
    for (oi_bps, equity, kappa, policy, cap) in [
        (10_000, u64::MAX, 10_000, u128::MAX, hard),
        (10_000, u64::MAX, 10_000, 1_000_000_007, 1_000_000_007),
        (5_000, 25_000_000_000, 1_000, u128::MAX, 2_500_000_000),
        (500, 25_000_000_000, 5_000, u128::MAX, 1_250_000_000),
    ] {
        let cap = u64::try_from(cap).unwrap();
        assert!(live(oi_bps, equity, kappa, policy, cap));
        assert!(!live(oi_bps, equity, kappa, policy, cap + 1));
    }
    for (oi_bps, equity, kappa, policy) in [
        (0, 1, 1, 1),
        (1, 0, 1, 1),
        (10_000, 10_000, 0, 1),
        (10_000, 10_000, 1, 0),
    ] {
        assert!(!live(oi_bps, equity, kappa, policy, 1));
    }
}

#[test]
fn conversion_and_error_vectors() {
    assert_eq!(live_point_cap(381_210_000), 1_143_630_000);
    assert_eq!(live_point_market_cap(381_210_000), 3_812_100_000);
    assert_eq!(
        sampled_floor_usdc(1_000_000_000, 100u128 << 64, 9).map(live_point_cap),
        Ok(300_000_000)
    );
    assert_eq!(floor_cap(1, 0, 1), Err(DiscoveryError::InvalidInput));
    assert_eq!(floor_cap(u128::MAX, 1, 1), Err(DiscoveryError::Overflow));
    assert_eq!(
        sampled_floor_usdc(u128::MAX, u128::MAX, 6),
        Err(DiscoveryError::InvalidInput)
    );
    assert_eq!(span(-1, 0), Err(DiscoveryError::InvalidInput));
    assert_eq!(span(0, i64::from(u32::MAX)), Ok((0, u32::MAX)));
    assert_eq!(live_point_cap(u128::MAX), u128::MAX);
    assert_eq!(floor_cap(1, 3, 2), Ok(1666));
}
