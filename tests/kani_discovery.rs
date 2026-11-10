#![cfg(kani)]

use colada_engine::discovery::*;

// Domain: u32 depth, concrete full policy multiple.
#[kani::proof]
#[kani::unwind(2)]
fn depth_budget_bounded_by_policy_cap() {
    let depth = u128::from(kani::any::<u32>());
    let cap = depth_budget(depth, DEPTH_MULTIPLE_BPS).unwrap();
    assert!(cap <= DEPTH_BUDGET_CAP);
    assert_eq!(cap, depth);
    kani::cover!(cap > 0);
}

// Domain: u32 depth, concrete single-row and multirow multiples.
#[kani::proof]
#[kani::unwind(2)]
fn depth_budget_single_row_no_greater_than_multirow() {
    let depth = u128::from(kani::any::<u32>());
    let single = depth_budget(depth, depth_multiple_bps(1)).unwrap();
    let multi = depth_budget(depth, depth_multiple_bps(2)).unwrap();
    assert!(single <= multi);
    assert!(single * 4 <= depth && depth < (single + 1) * 4);
    kani::cover!(single > 0);
}

// Domain: ordered u16 floors, either cap mode; saturation boundaries are concrete.
#[kani::proof]
#[kani::unwind(2)]
fn point_caps_monotone_and_ordered() {
    let a = kani::any::<u16>();
    let b = kani::any::<u16>();
    kani::assume(a <= b);
    let lifted = kani::any::<bool>();
    let x = point_caps(u128::from(a), lifted);
    let y = point_caps(u128::from(b), lifted);
    assert!(x.0 <= y.0 && x.1 <= y.1 && x.0 <= x.1 && y.0 <= y.1);
    assert_eq!(live_point_cap(u128::MAX), u128::MAX);
    assert_eq!(live_point_market_cap(u128::MAX), u128::MAX);
    kani::cover!(x.0 > 0 && x.0 < y.0);
}

// Domain: u16 floor, concrete Q64 price and nine decimals (quotient floor / 10).
#[kani::proof]
#[kani::unwind(6)]
fn sampled_floor_conversion_rounds_down() {
    let floor = u128::from(kani::any::<u16>());
    let cap = sampled_floor_usdc(floor, 100_u128 << 64, 9).unwrap();
    assert!(cap * 10 <= floor && floor < (cap + 1) * 10);
    kani::cover!(cap > 0 && cap * 10 < floor);
}

// Domain: positive u16 floor; concrete deviation/safety families avoid symbolic wide division.
#[kani::proof]
#[kani::unwind(2)]
fn robust_cap_tightens_with_deviation_and_safety() {
    let floor = u128::from(kani::any::<u16>());
    kani::assume(floor > 0);
    let base = floor_cap(floor, 50, 2).unwrap();
    let deviation = floor_cap(floor, 100, 2).unwrap();
    let safety = floor_cap(floor, 50, 4).unwrap();
    assert!(deviation <= base && safety <= base);
    assert_eq!(base, floor * 100);
    assert_eq!(deviation, floor * 50);
    assert_eq!(safety, floor * 50);
    kani::cover!(base > deviation);
}

// Domain: equity 10000..75535, policy 1..65536, concrete 10% vault and 20% closeout.
#[kani::proof]
#[kani::unwind(2)]
fn live_notional_rejects_above_any_cap() {
    let equity = u64::from(kani::any::<u16>()) + 10_000;
    let policy = u128::from(kani::any::<u16>()) + 1;
    let cap = (u128::from(equity) / 10).min(policy) as u64;
    assert_eq!(
        validate_live_notional(2000, equity, 1000, policy, cap),
        Ok(())
    );
    assert_eq!(
        validate_live_notional(2000, equity, 1000, policy, cap + 1),
        Err(DiscoveryError::BoundsExceeded)
    );
    kani::cover!(cap > 0);
}

// Domain: u16 start and u8 common duration, two supported legs including a sampled leg.
#[kani::proof]
#[kani::unwind(4)]
fn sampled_support_requires_120s_common_overlap() {
    let start = u32::from(kani::any::<u16>());
    let duration = u32::from(kani::any::<u8>());
    let support = Some((start, start + duration));
    let result = overlap(&[(support, true), (support, false)]);
    if duration >= 120 {
        assert_eq!(result, Ok(support));
    } else {
        assert_eq!(result, Err(DiscoveryError::InvalidInput));
    }
    kani::cover!(result.is_ok());
}

// Domain: positive u16 floor, each zero parameter; explicit overflow and unit boundaries.
#[kani::proof]
#[kani::unwind(2)]
fn zero_floor_or_zero_cap_parameters_rejected() {
    let floor = u128::from(kani::any::<u16>()) + 1;
    assert_eq!(floor_cap(0, 1, 1), Err(DiscoveryError::InvalidInput));
    assert_eq!(floor_cap(floor, 0, 1), Err(DiscoveryError::InvalidInput));
    assert_eq!(floor_cap(floor, 1, 0), Err(DiscoveryError::InvalidInput));
    assert_eq!(floor_cap(u128::MAX, 1, 1), Err(DiscoveryError::Overflow));
    assert_eq!(floor_cap(1, 1, 1), Ok(10_000));
    let cap = floor_cap(floor, 1, 1).unwrap();
    assert_eq!(cap, floor * 10_000);
    kani::cover!(cap > 0);
}
