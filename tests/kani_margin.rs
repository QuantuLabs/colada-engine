#![cfg(kani)]

use colada_engine::margin::*;

// Domain: all u16 requested and market rates.
#[kani::proof]
#[kani::unwind(2)]
fn margin_requested_current_policy() {
    let requested: u16 = kani::any();
    let market: u16 = kani::any();
    let result = validate_requested_initial_margin(requested, market);
    let accepted = requested == market || (requested >= market && requested <= 5_000);
    assert_eq!(
        result,
        if accepted {
            Ok(())
        } else {
            Err(MarginError::InitialMarginOutOfRange)
        }
    );
    kani::cover!(result.is_ok() && requested > 5_000);
}

// Family: margin requirement is the exact ceiling — instances: rates 500 and 200 bps.
macro_rules! ceiling_property {
    ($( $(#[$attr:meta])* fn $name:ident($rate:expr); )*) => { $(
        $(#[$attr])*
        fn $name() {
            let n = u64::from(kani::any::<u16>());
            let result = margin_requirement_ceil(n, $rate).unwrap();
            let product = n * u64::from($rate);
            assert!(result * 10_000 >= product);
            assert!(result == 0 || (result - 1) * 10_000 < product);
            assert_eq!(initial_margin_requirement(n, $rate), Ok(result));
            kani::cover!(result > 0);
        }
    )* };
}
ceiling_property! {
    // Domain: u16 notional widened to u64, concrete basis-point rate.
    #[kani::proof]
    #[kani::unwind(2)]
    fn margin_requirement_is_ceiling_500(500u16);
    #[kani::proof]
    #[kani::unwind(2)]
    fn margin_requirement_is_ceiling_200(200u16);
}

// Domain: ordered u16 notionals, fixed 500 basis-point rate.
#[kani::proof]
#[kani::unwind(2)]
fn margin_requirement_monotone_notional() {
    let a: u16 = kani::any();
    let b: u16 = kani::any();
    kani::assume(a <= b);
    let low = initial_margin_requirement(u64::from(a), 500).unwrap();
    let high = initial_margin_requirement(u64::from(b), 500).unwrap();
    assert!(low <= high);
    kani::cover!(low < high);
}

// Domain: full oracle setting ranges and version, fixed baseline pair.
#[kani::proof]
#[kani::unwind(2)]
fn baseline_margin_pair_preserved() {
    let enabled: bool = kani::any();
    let age: i64 = kani::any();
    let confidence: u16 = kani::any();
    let version: u16 = kani::any();
    assert!(margin_pair_admissible(500, 250, age, confidence));
    let result = is_margin_pair_admissible(500, 250, enabled, age, confidence, version);
    assert!(result);
    kani::cover!(result);
}

// Domain: full oracle setting ranges and version, fixed lower-margin pair.
#[kani::proof]
#[kani::unwind(2)]
fn tier1_requires_strict_oracle_bounds_and_template() {
    let enabled: bool = kani::any();
    let age: i64 = kani::any();
    let confidence: u16 = kani::any();
    let version: u16 = kani::any();
    let bounds = (1..=2).contains(&age) && (1..=100).contains(&confidence);
    assert_eq!(margin_pair_admissible(200, 100, age, confidence), bounds);
    let result = is_margin_pair_admissible(200, 100, enabled, age, confidence, version);
    assert_eq!(result, bounds && enabled && version >= 1);
    kani::cover!(result);
}

// Domain: all u16 margin pairs, concrete admissible oracle settings.
#[kani::proof]
#[kani::unwind(2)]
fn invalid_margin_pairs_fail_closed() {
    let initial: u16 = kani::any();
    let maintenance: u16 = kani::any();
    let permitted = (initial, maintenance) == (500, 250) || (initial, maintenance) == (200, 100);
    let result = is_margin_pair_admissible(initial, maintenance, true, 2, 100, 1);
    assert_eq!(result, permitted);
    assert_eq!(
        margin_pair_admissible(initial, maintenance, 2, 100),
        permitted
    );
    kani::cover!(result);
}

// Domain: concrete 0, 1 and MAX arithmetic boundaries.
#[kani::proof]
#[kani::unwind(2)]
fn margin_requirement_boundaries() {
    assert_eq!(margin_requirement_ceil(0, u16::MAX), Ok(0));
    assert_eq!(margin_requirement_ceil(1, 1), Ok(1));
    let result = margin_requirement_ceil(u64::MAX, 10_000);
    assert_eq!(result, Ok(u64::MAX));
    assert_eq!(
        margin_requirement_ceil(u64::MAX, u16::MAX),
        Err(MarginError::Overflow)
    );
    kani::cover!(result.is_ok());
}
