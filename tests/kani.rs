//! Bounded current-kernel properties; family instances are separate exact proofs.
//! Explicit case selectors enumerate finite inputs before calling the production API.
//! No full-envelope or full-old-domain proof is claimed; see each family domain.
//! Run one named harness at a time through the memory-bounded parent script.
#![cfg(kani)]

use colada_engine::math::funded_net_and_capped;
use colada_engine::math::{
    atoms_debit_ceil_wide, atoms_payout_floor_wide, checked_price_bounds, compose_dispersion_bps,
    mul_q64_ceil, mul_q64_floor, quote_e6_floor_wide, secure_reference_step, MathError,
    SecureReferenceVNext, SecureStepMode, U256,
};

#[kani::proof]
#[kani::unwind(70)]
fn funded_net_respects_caps() {
    let size: i64 = kani::any();
    let entry: u64 = kani::any();
    let exit: u64 = kani::any();
    let checkpoint: i128 = kani::any();
    let current: i128 = kani::any();
    let allocation: u64 = kani::any();
    let credit: bool = kani::any();
    kani::assume(size != 0 && (-1000..=1000).contains(&size));
    kani::assume((1..=10000).contains(&entry) && (1..=10000).contains(&exit));
    kani::assume((-8..=8).contains(&checkpoint) && (-8..=8).contains(&current));
    kani::assume(allocation <= 1000);
    let (_, capped) = funded_net_and_capped(
        size, entry, exit, checkpoint * (1_i128 << 64), current * (1_i128 << 64),
        allocation, credit,
    ).unwrap();
    assert!(capped >= -i128::from(allocation));
    assert!(capped <= i128::from(size.unsigned_abs()));
}

#[kani::proof]
#[kani::unwind(5)]
fn q64_floor_ceil_bounded_product() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    let x: u16 = kani::any();
    let y: u16 = kani::any();
    kani::assume((1..=16).contains(&a) && (1..=16).contains(&b));
    let (a, b, x, y) = (u128::from(a), u128::from(b), u128::from(x), u128::from(y));
    let q = 1_u128 << 64;
    let left = a * q + x;
    let right = b * q + y;
    let base = a * b * q + a * y + b * x;
    let rem = x * y;
    let floor = mul_q64_floor(left, right).unwrap();
    let ceil = mul_q64_ceil(left, right).unwrap();
    assert_eq!(floor, base);
    assert_eq!(ceil, base + u128::from(rem != 0));
    assert!(ceil - floor <= 1);
}

macro_rules! composed_dispersion_nondecreasing_family {
    ($( $(#[$attr:meta])* fn $name:ident($fixed:expr, $left:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let a: u16 = kani::any();
                let b: u16 = kani::any();
                let fixed: u16 = $fixed;
                kani::assume(a <= b && b <= 4000);
                let (l0, r0, l1, r1) = if $left { (a, fixed, b, fixed) } else { (fixed, a, fixed, b) };
                let d0 = compose_dispersion_bps(l0, r0).unwrap();
                let d1 = compose_dispersion_bps(l1, r1).unwrap();
                assert!(d0 <= d1);
                assert!(d0 >= l0.max(r0));
                assert!(d1 >= l1.max(r1));
                assert!(d1 <= 10000);
                kani::cover!(a < b);
            }
        )*
    };
}

// Family: composed_dispersion_nondecreasing — instances: left,right × opposite0,1,4000 (6)
composed_dispersion_nondecreasing_family! {
    #[kani::proof]
    #[kani::unwind(2)]
    fn composed_dispersion_nondecreasing_left_fixed0(0, true);
    #[kani::proof]
    #[kani::unwind(2)]
    fn composed_dispersion_nondecreasing_left_fixed1(1, true);
    #[kani::proof]
    #[kani::unwind(2)]
    fn composed_dispersion_nondecreasing_left_fixed4000(4000, true);
    #[kani::proof]
    #[kani::unwind(2)]
    fn composed_dispersion_nondecreasing_right_fixed0(0, false);
    #[kani::proof]
    #[kani::unwind(2)]
    fn composed_dispersion_nondecreasing_right_fixed1(1, false);
    #[kani::proof]
    #[kani::unwind(2)]
    fn composed_dispersion_nondecreasing_right_fixed4000(4000, false);
}

#[kani::proof]
#[kani::unwind(2)]
fn debit_only_fractional_funding_rounds_against_payer() {
    let n: u8 = kani::any();
    let whole: u8 = kani::any();
    let fraction: u16 = kani::any();
    let long: bool = kani::any();
    let positive_delta: bool = kani::any();
    kani::assume((1..=16).contains(&n));
    kani::assume(whole <= 4);
    kani::assume((1..=1000).contains(&fraction));
    let size = if long { i64::from(n) } else { -i64::from(n) };
    let delta = (i128::from(whole) << 64) + i128::from(fraction);
    let current = if positive_delta { delta } else { -delta };
    let expected = if long == positive_delta {
        -(i128::from(n) * i128::from(whole) + 1)
    } else {
        0
    };
    let (raw, capped) = funded_net_and_capped(size, 1, 1, 0, current, 100, false).unwrap();
    assert_eq!(raw, expected);
    assert_eq!(capped, expected);
}

#[kani::proof]
#[kani::unwind(2)]
fn secure_reference_frozen_and_zero_elapsed() {
    let mode_id: u8 = kani::any();
    let base: u16 = kani::any();
    let p: u16 = kani::any();
    let frac: u16 = kani::any();
    let s0: u16 = kani::any();
    let t0: i16 = kani::any();
    let ds: u8 = kani::any();
    let du: u8 = kani::any();
    let axis: bool = kani::any();
    let lifted: bool = kani::any();
    kani::assume(mode_id <= 2);
    kani::assume(base >= 1 && p >= 1);
    let mode = match mode_id {
        0 => SecureStepMode::Frozen,
        1 => SecureStepMode::Fresh,
        _ => SecureStepMode::Open,
    };
    let r = SecureReferenceVNext {
        ref_q32: (u128::from(base) << 32) + u128::from(frac),
        slot: u64::from(s0),
        unix: i64::from(t0),
        lifted,
    };
    let nextslot = r.slot
        + if mode == SecureStepMode::Frozen || !axis {
            u64::from(ds)
        } else {
            0
        };
    let nextunix = r.unix
        + if mode == SecureStepMode::Frozen || axis {
            i64::from(du)
        } else {
            0
        };
    let out = secure_reference_step(r, u64::from(p), nextslot, nextunix, mode);
    assert_eq!(out.ref_q32, r.ref_q32);
    assert_eq!(out.lifted, r.lifted);
    if mode == SecureStepMode::Frozen
        || (mode == SecureStepMode::Fresh && (u128::from(p) << 32) != r.ref_q32)
    {
        assert_eq!(out.slot, r.slot);
        assert_eq!(out.unix, r.unix);
    } else {
        assert_eq!(out.slot, nextslot);
        assert_eq!(out.unix, nextunix);
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn conversion_validation_precedence() {
    let n: u64 = kani::any();
    let lo: u128 = kani::any();
    let hi: u128 = kani::any();
    let d: u8 = kani::any();
    let w = U256::from_limbs_le([n, 0, 0, u64::MAX]);
    let expected = if (6..=9).contains(&d) {
        MathError::ZeroPrice
    } else {
        MathError::InvalidDecimals
    };
    assert!(matches!(quote_e6_floor_wide(n, 0, d), Err(error) if error == expected));
    assert!(matches!(atoms_debit_ceil_wide(w, 0, d), Err(error) if error == expected));
    assert!(matches!(
        atoms_debit_ceil_wide(w, 1, 6),
        Err(MathError::OutOfEnvelope)
    ));
    let bounds = checked_price_bounds(lo, hi);
    if lo == 0 || hi == 0 {
        assert!(matches!(bounds, Err(MathError::ZeroPrice)));
    } else if lo > hi {
        assert!(matches!(bounds, Err(MathError::InvertedPriceBounds)));
    } else {
        assert!(bounds.is_ok());
    }
}

// These two identity obligations compose; each calls the actual production API independently.
#[kani::proof]
#[kani::unwind(33)]
fn identity_quote_exact() {
    let amount: u16 = kani::any();
    kani::assume((1..=10000).contains(&amount));
    let amount = u64::from(amount);
    let actual = quote_e6_floor_wide(amount, 1_u128 << 64, 6).unwrap();
    assert_eq!(actual.try_u64().unwrap(), amount);
}

macro_rules! identity_inverse_exact_family {
    ($( $(#[$attr:meta])* fn $name:ident($debit:expr, $amount:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let amount: u16 = $amount;
                let debit: bool = $debit;
                kani::assume((1..=10000).contains(&amount));
                let amount = u64::from(amount);
                let q = U256::from_u64(amount);
                let actual = if debit {
                    atoms_debit_ceil_wide(q, 1_u128 << 64, 6)
                } else {
                    atoms_payout_floor_wide(q, 1_u128 << 64, 6)
                }
                .unwrap();
                assert_eq!(actual.try_u64().unwrap(), amount);
                kani::cover!(amount >= 1);
            }
        )*
    };
}

// Family: identity_inverse_exact — instances: payout amount1..10000 (1)
identity_inverse_exact_family! {
    #[kani::proof]
    #[kani::unwind(11)]
    fn identity_inverse_exact_payout(false, kani::any::<u16>());
}

macro_rules! identity_inverse_exact_debit_case {
    ($amount:expr) => {{
        let amount: u16 = $amount;
        let debit: bool = true;
        kani::assume((1..=10000).contains(&amount));
        let amount = u64::from(amount);
        let q = U256::from_u64(amount);
        let actual = if debit {
            atoms_debit_ceil_wide(q, 1_u128 << 64, 6)
        } else {
            atoms_payout_floor_wide(q, 1_u128 << 64, 6)
        }
        .unwrap();
        assert_eq!(actual.try_u64().unwrap(), amount);
    }};
}

macro_rules! identity_inverse_exact_debit_family {
    ($( $(#[$attr:meta])* fn $name:ident(); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 2);
                match case_id {
                    0 => identity_inverse_exact_debit_case!(1),
                    1 => identity_inverse_exact_debit_case!(10000),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: identity_inverse_exact_debit — instances: amount1,10000, enumerated within one proof (1)
identity_inverse_exact_debit_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn identity_inverse_exact_debit_boundaries();
}

// Final bounded atoms domain: 0,1,5,65535; all other atoms are unproved here.
macro_rules! collateral_fractional_value_haircut_and_impairment_case {
    ($k:expr, $d:expr, $a:expr) => {{
        let a: u16 = $a;
        let k: u8 = $k;
        let d: u8 = $d;
        // Witnesses: (a,k,d)=(5,3,6) -> (3,1,1); (1,1,9) -> (0,0,1).
        kani::assume(1 <= k && k <= 31);
        kani::assume(6 <= d && d <= 9);
        let z = match d { 6 => 1_u64, 7 => 10, 8 => 100, 9 => 1000, _ => unreachable!() };
        let q = 1_u128 << 64;
        let p = u128::from(k) * q / 4;
        let hi = (u128::from(k) + 1) * q / 4;
        let num = u64::from(a) * u64::from(k);
        let den = 4 * z;
        let v = num / den;
        let bounds = colada_engine::math::checked_price_bounds(p, hi).unwrap();
        assert_eq!(colada_engine::math::quote_e6_floor_wide(u64::from(a), p, d).unwrap().try_u64().unwrap(), v);
        assert_eq!(colada_engine::math::credited_quote_e6_wide(u64::from(a), p, d).unwrap().try_u64().unwrap(), v / 2);
        assert_eq!(colada_engine::math::displayed_subatom_impairment_bound_quote_e6(bounds, d).unwrap().try_u64().unwrap(), (u64::from(k) + 1 + den - 1) / den);
    }};
}

macro_rules! collateral_fractional_value_haircut_and_impairment_family {
    ($( $(#[$attr:meta])* fn $name:ident($k:expr, $d:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 4);
                match case_id {
                    0 => collateral_fractional_value_haircut_and_impairment_case!($k, $d, 0),
                    1 => collateral_fractional_value_haircut_and_impairment_case!($k, $d, 1),
                    2 => collateral_fractional_value_haircut_and_impairment_case!($k, $d, 5),
                    3 => collateral_fractional_value_haircut_and_impairment_case!($k, $d, 65535),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: collateral_fractional_value_haircut_and_impairment — instances: k1,k3,k16,k31 × d6..d9; each enumerates atoms0,1,5,65535 (16)
collateral_fractional_value_haircut_and_impairment_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k1_d6(1, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k1_d7(1, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k1_d8(1, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k1_d9(1, 9);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k3_d6(3, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k3_d7(3, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k3_d8(3, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k3_d9(3, 9);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k16_d6(16, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k16_d7(16, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k16_d8(16, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k16_d9(16, 9);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k31_d6(31, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k31_d7(31, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k31_d8(31, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_fractional_value_haircut_and_impairment_k31_d9(31, 9);
}

macro_rules! collateral_inverse_fractional_rounding_case {
    ($k:expr, $d:expr, $debit:expr, $q:expr) => {{
        let q: u16 = $q;
        let k: u8 = $k;
        let d: u8 = $d;
        let debit: bool = $debit;
        // Witnesses: q=1,k=3,d=6 -> payout1/debit2; q=3 -> both4.
        kani::assume(q <= 1000);
        kani::assume(1 <= k && k <= 16);
        kani::assume(6 <= d && d <= 9);
        let z = match d { 6 => 1_u64, 7 => 10, 8 => 100, 9 => 1000, _ => unreachable!() };
        let p = u128::from(k) * (1_u128 << 64) / 4;
        let n = u64::from(q) * 4 * z;
        let quote = colada_engine::math::U256::from_u64(u64::from(q));
        let a = if debit {
            colada_engine::math::atoms_debit_ceil_wide(quote, p, d)
        } else {
            colada_engine::math::atoms_payout_floor_wide(quote, p, d)
        }.unwrap().try_u64().unwrap();
        if debit {
            assert!(a * u64::from(k) >= n);
            assert!(a == 0 || (a - 1) * u64::from(k) < n);
        } else {
            assert!(a * u64::from(k) <= n);
            assert!((a + 1) * u64::from(k) > n);
        }
    }};
}

macro_rules! collateral_inverse_fractional_rounding_family {
    ($( $(#[$attr:meta])* fn $name:ident($k:expr, $d:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let q: u16 = kani::any();
                collateral_inverse_fractional_rounding_case!($k, $d, false, q);
                kani::cover!(q > 0);
            }
        )*
    };
}

// Family: collateral_inverse_fractional_rounding — instances: k1,k3,k16 × d6..d9 × payout quote0..1000 (12)
collateral_inverse_fractional_rounding_family! {
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k1_d6_payout(1, 6);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k1_d7_payout(1, 7);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k1_d8_payout(1, 8);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k1_d9_payout(1, 9);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k3_d6_payout(3, 6);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k3_d7_payout(3, 7);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k3_d8_payout(3, 8);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k3_d9_payout(3, 9);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k16_d6_payout(16, 6);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k16_d7_payout(16, 7);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k16_d8_payout(16, 8);
    #[kani::proof]
    #[kani::unwind(11)]
    fn collateral_inverse_fractional_rounding_k16_d9_payout(16, 9);
}

macro_rules! collateral_inverse_fractional_rounding_debit_case {
    ($k:expr, $d:expr, $q:expr) => {{
        collateral_inverse_fractional_rounding_case!($k, $d, true, $q);
    }};
}

macro_rules! collateral_inverse_fractional_rounding_debit_family {
    ($( $(#[$attr:meta])* fn $name:ident($k:expr, $d:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 4);
                match case_id {
                    0 => collateral_inverse_fractional_rounding_debit_case!($k, $d, 0),
                    1 => collateral_inverse_fractional_rounding_debit_case!($k, $d, 1),
                    2 => collateral_inverse_fractional_rounding_debit_case!($k, $d, 3),
                    3 => collateral_inverse_fractional_rounding_debit_case!($k, $d, 1000),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: collateral_inverse_fractional_rounding_debit — instances: k1,k3,k16 × d6..d9; each enumerates quote0,1,3,1000 (12)
collateral_inverse_fractional_rounding_debit_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k1_d6(1, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k1_d7(1, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k1_d8(1, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k1_d9(1, 9);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k3_d6(3, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k3_d7(3, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k3_d8(3, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k3_d9(3, 9);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k16_d6(16, 6);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k16_d7(16, 7);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k16_d8(16, 8);
    #[kani::proof]
    #[kani::unwind(33)]
    fn collateral_inverse_fractional_rounding_debit_k16_d9(16, 9);
}

#[kani::proof]
#[kani::unwind(5)]
fn authenticated_cap_and_u256_narrowing_full_limbs() {
    let limbs: [u64; 4] = kani::any();
    let cap: u64 = kani::any();
    // Witnesses: [0,0,0,1],cap=MAX; [7,0,0,0],cap=10; cap=0.
    let wide = limbs[1] != 0 || limbs[2] != 0 || limbs[3] != 0;
    let value = colada_engine::math::U256::from_limbs_le(limbs);
    let actual = colada_engine::math::cap_quote_wide(value, colada_engine::math::AuthenticatedQuoteCap::from_authenticated_parent(cap));
    assert_eq!(actual, if wide { cap } else { limbs[0].min(cap) });
    assert!(actual <= cap);
    if wide {
        assert!(matches!(value.try_u64(), Err(colada_engine::math::MathError::InvalidNarrowing)));
    } else {
        assert_eq!(value.try_u64().unwrap(), limbs[0]);
    }
}

#[kani::proof]
#[kani::unwind(161)]
fn open_allocation_availability_and_narrowing_precedence() {
    let m: u16 = kani::any();
    let free: u64 = kani::any();
    let d: u8 = kani::any();
    let wide: bool = kani::any();
    // Witnesses: m1,d6,!wide,free1 rejects/free2 succeeds; wide,d9,freeMAX rejects narrowing.
    kani::assume(m <= 1000);
    let z = match d { 7 => 10_u64, 8 => 100, 9 => 1000, _ => 1 };
    let p = u128::from(z) * (1_u128 << 64);
    let bounds = colada_engine::math::checked_price_bounds(p, u128::MAX).unwrap();
    let margin = if wide { 1_u64 << 63 } else { u64::from(m) };
    let actual = colada_engine::math::open_pre_haircut_requirement(margin, bounds, d, free);
    if !(6..=9).contains(&d) {
        assert!(matches!(actual, Err(colada_engine::math::MathError::InvalidDecimals)));
    } else if wide {
        assert!(matches!(actual, Err(colada_engine::math::MathError::InvalidNarrowing)));
    } else {
        let need = 2 * u64::from(m);
        if free < need {
            assert!(matches!(actual, Err(colada_engine::math::MathError::InsufficientFreeAtoms)));
        } else {
            let out = actual.unwrap();
            assert_eq!(out.required_allocation_atoms, need);
            assert_eq!(out.required_pre_haircut_quote_e6_wide.try_u64().unwrap(), need);
        }
    }
}

// Additional positive subsets; the full-domain obligation above remains inconclusive.
macro_rules! open_allocation_availability_and_narrowing_precedence_normal_case {
    ($d:expr, $wide:expr, $m:expr) => {{
        let m: u16 = $m;
        let free: u64 = kani::any();
        let d: u8 = $d;
        let wide: bool = $wide;
        // Witnesses: m1,d6,!wide,free1 rejects/free2 succeeds; wide,d9,freeMAX rejects narrowing.
        kani::assume(m <= 1000);
        let z = match d { 7 => 10_u64, 8 => 100, 9 => 1000, _ => 1 };
        let p = u128::from(z) * (1_u128 << 64);
        let bounds = colada_engine::math::checked_price_bounds(p, u128::MAX).unwrap();
        let margin = if wide { 1_u64 << 63 } else { u64::from(m) };
        let actual = colada_engine::math::open_pre_haircut_requirement(margin, bounds, d, free);
        if !(6..=9).contains(&d) {
            assert!(matches!(actual, Err(colada_engine::math::MathError::InvalidDecimals)));
        } else if wide {
            assert!(matches!(actual, Err(colada_engine::math::MathError::InvalidNarrowing)));
        } else {
            let need = 2 * u64::from(m);
            if free < need {
                assert!(matches!(actual, Err(colada_engine::math::MathError::InsufficientFreeAtoms)));
            } else {
                let out = actual.unwrap();
                assert_eq!(out.required_allocation_atoms, need);
                assert_eq!(out.required_pre_haircut_quote_e6_wide.try_u64().unwrap(), need);
            }
        }
    }};
}

macro_rules! open_allocation_availability_and_narrowing_precedence_normal_family {
    ($( $(#[$attr:meta])* fn $name:ident($d:expr, $wide:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 3);
                match case_id {
                    0 => open_allocation_availability_and_narrowing_precedence_normal_case!($d, $wide, 0),
                    1 => open_allocation_availability_and_narrowing_precedence_normal_case!($d, $wide, 1),
                    2 => open_allocation_availability_and_narrowing_precedence_normal_case!($d, $wide, 1000),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: open_allocation_availability_and_narrowing_precedence_normal — instances: d6..d9; margin0,1,1000 per proof, full free u64 (4)
open_allocation_availability_and_narrowing_precedence_normal_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn open_allocation_availability_and_narrowing_precedence_normal_d6(6, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn open_allocation_availability_and_narrowing_precedence_normal_d7(7, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn open_allocation_availability_and_narrowing_precedence_normal_d8(8, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn open_allocation_availability_and_narrowing_precedence_normal_d9(9, false);
}



macro_rules! open_allocation_availability_and_narrowing_precedence_invalid_case {
    ($wide:expr, $m:expr, $d:expr) => {{
        let m: u16 = $m;
        let free: u64 = kani::any();
        let d: u8 = $d;
        let wide: bool = $wide;
        // Witnesses: m1,d6,!wide,free1 rejects/free2 succeeds; wide,d9,freeMAX rejects narrowing.
        kani::assume(m <= 1000);
        let z = match d { 7 => 10_u64, 8 => 100, 9 => 1000, _ => 1 };
        let p = u128::from(z) * (1_u128 << 64);
        let bounds = colada_engine::math::checked_price_bounds(p, u128::MAX).unwrap();
        let margin = if wide { 1_u64 << 63 } else { u64::from(m) };
        let actual = colada_engine::math::open_pre_haircut_requirement(margin, bounds, d, free);
        if !(6..=9).contains(&d) {
            assert!(matches!(actual, Err(colada_engine::math::MathError::InvalidDecimals)));
        } else if wide {
            assert!(matches!(actual, Err(colada_engine::math::MathError::InvalidNarrowing)));
        } else {
            let need = 2 * u64::from(m);
            if free < need {
                assert!(matches!(actual, Err(colada_engine::math::MathError::InsufficientFreeAtoms)));
            } else {
                let out = actual.unwrap();
                assert_eq!(out.required_allocation_atoms, need);
                assert_eq!(out.required_pre_haircut_quote_e6_wide.try_u64().unwrap(), need);
            }
        }
    }};
}

macro_rules! open_allocation_availability_and_narrowing_precedence_invalid_family {
    ($( $(#[$attr:meta])* fn $name:ident($wide:expr, $m:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 4);
                match case_id {
                    0 => open_allocation_availability_and_narrowing_precedence_invalid_case!($wide, $m, 0),
                    1 => open_allocation_availability_and_narrowing_precedence_invalid_case!($wide, $m, 5),
                    2 => open_allocation_availability_and_narrowing_precedence_invalid_case!($wide, $m, 10),
                    3 => open_allocation_availability_and_narrowing_precedence_invalid_case!($wide, $m, 255),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: open_allocation_availability_and_narrowing_precedence_invalid — instances: invalid d0,5,10,255, full free/wide/margin selector domains (1)
open_allocation_availability_and_narrowing_precedence_invalid_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn open_allocation_availability_and_narrowing_precedence_invalid_boundaries(kani::any::<bool>(), kani::any::<u16>());
}

macro_rules! final_debit_partition_exact_cover_and_wide_clamp_normal_case {
    ($wide:expr, $loss:expr, $eligible:expr, $eligible_wide:expr) => {{
        let loss: u8 = $loss;
        let eligible: u8 = $eligible;
        let eligible_wide: u64 = $eligible_wide;
        let wide: bool = $wide;
        // Witnesses: normal L2,E1 -> (2,1,1,1); L3,E2 covers3; wide E0/MAX uncovers1.
        let q = 1_u128 << 64;
        let p = if wide { 1 } else { 3 * q / 2 };
        let d = if wide { 9 } else { 6 };
        let l = if wide { 1 } else { u64::from(loss) };
        let e = if wide { eligible_wide } else { u64::from(eligible) };
        let bounds = colada_engine::math::checked_price_bounds(p, p).unwrap();
        let out = colada_engine::math::final_debit_partition(l, bounds, d, e).unwrap();
        if wide {
            assert_eq!(out.theoretical_debit_atoms_wide, colada_engine::math::U256::from_u128(1000 * q));
            assert_eq!(out.debited_atoms, e);
            assert_eq!(out.covered_loss_quote_e6, 0);
            assert_eq!(out.uncovered_loss_quote_e6, 1);
        } else {
            let t = (2 * l + 2) / 3;
            let covered = if t <= e { l } else { 3 * e / 2 };
            assert_eq!(out.theoretical_debit_atoms_wide.try_u64().unwrap(), t);
            assert_eq!(out.debited_atoms, t.min(e));
            assert_eq!(out.covered_loss_quote_e6, covered);
            assert_eq!(out.uncovered_loss_quote_e6, l - covered);
        }
        assert!(out.debited_atoms <= e);
        assert!(out.covered_loss_quote_e6 <= l);
        assert_eq!(out.covered_loss_quote_e6 + out.uncovered_loss_quote_e6, l);
    }};
}

macro_rules! final_debit_partition_exact_cover_and_wide_clamp_normal_family {
    ($( $(#[$attr:meta])* fn $name:ident($wide:expr, $loss:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 4);
                match case_id {
                    0 => final_debit_partition_exact_cover_and_wide_clamp_normal_case!($wide, $loss, 0, 0),
                    1 => final_debit_partition_exact_cover_and_wide_clamp_normal_case!($wide, $loss, 1, 0),
                    2 => final_debit_partition_exact_cover_and_wide_clamp_normal_case!($wide, $loss, 2, 0),
                    3 => final_debit_partition_exact_cover_and_wide_clamp_normal_case!($wide, $loss, 255, 0),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: final_debit_partition_exact_cover_and_wide_clamp_normal — instances: loss0,1,2,3,255; eligible0,1,2,255 per instance (5)
final_debit_partition_exact_cover_and_wide_clamp_normal_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn final_debit_partition_exact_cover_and_wide_clamp_normal_l0(false, 0);
    #[kani::proof]
    #[kani::unwind(33)]
    fn final_debit_partition_exact_cover_and_wide_clamp_normal_l1(false, 1);
    #[kani::proof]
    #[kani::unwind(33)]
    fn final_debit_partition_exact_cover_and_wide_clamp_normal_l2(false, 2);
    #[kani::proof]
    #[kani::unwind(33)]
    fn final_debit_partition_exact_cover_and_wide_clamp_normal_l3(false, 3);
    #[kani::proof]
    #[kani::unwind(33)]
    fn final_debit_partition_exact_cover_and_wide_clamp_normal_l255(false, 255);
}

macro_rules! final_debit_partition_exact_cover_and_wide_clamp_wide_case {
    ($wide:expr, $loss:expr, $eligible:expr, $eligible_wide:expr) => {{
        let loss: u8 = $loss;
        let eligible: u8 = $eligible;
        let eligible_wide: u64 = $eligible_wide;
        let wide: bool = $wide;
        // Witnesses: normal L2,E1 -> (2,1,1,1); L3,E2 covers3; wide E0/MAX uncovers1.
        let q = 1_u128 << 64;
        let p = if wide { 1 } else { 3 * q / 2 };
        let d = if wide { 9 } else { 6 };
        let l = if wide { 1 } else { u64::from(loss) };
        let e = if wide { eligible_wide } else { u64::from(eligible) };
        let bounds = colada_engine::math::checked_price_bounds(p, p).unwrap();
        let out = colada_engine::math::final_debit_partition(l, bounds, d, e).unwrap();
        if wide {
            assert_eq!(out.theoretical_debit_atoms_wide, colada_engine::math::U256::from_u128(1000 * q));
            assert_eq!(out.debited_atoms, e);
            assert_eq!(out.covered_loss_quote_e6, 0);
            assert_eq!(out.uncovered_loss_quote_e6, 1);
        } else {
            let t = (2 * l + 2) / 3;
            let covered = if t <= e { l } else { 3 * e / 2 };
            assert_eq!(out.theoretical_debit_atoms_wide.try_u64().unwrap(), t);
            assert_eq!(out.debited_atoms, t.min(e));
            assert_eq!(out.covered_loss_quote_e6, covered);
            assert_eq!(out.uncovered_loss_quote_e6, l - covered);
        }
        assert!(out.debited_atoms <= e);
        assert!(out.covered_loss_quote_e6 <= l);
        assert_eq!(out.covered_loss_quote_e6 + out.uncovered_loss_quote_e6, l);
    }};
}

macro_rules! final_debit_partition_exact_cover_and_wide_clamp_wide_family {
    ($( $(#[$attr:meta])* fn $name:ident($wide:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 3);
                match case_id {
                    0 => final_debit_partition_exact_cover_and_wide_clamp_wide_case!($wide, 0, 0, 0),
                    1 => final_debit_partition_exact_cover_and_wide_clamp_wide_case!($wide, 0, 0, 1),
                    2 => final_debit_partition_exact_cover_and_wide_clamp_wide_case!($wide, 0, 0, u64::MAX),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: final_debit_partition_exact_cover_and_wide_clamp_wide — instances: eligible0,1,MAX, 3 cases (1)
final_debit_partition_exact_cover_and_wide_clamp_wide_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn final_debit_partition_exact_cover_and_wide_clamp_wide_boundaries(true);
}

#[kani::proof]
#[kani::unwind(33)]
fn u256_add_and_double_carry_overflow() {
    let lo: u64 = kani::any();
    let top: u64 = kani::any();
    let step: u8 = kani::any();
    let multiply: bool = kani::any();
    // Witnesses: add loMAX,step1,top0 -> [0,0,0,1]; topMAX overflows.
    // Double top=(1<<63)-1 succeeds; top=1<<63 overflows.
    let m = u64::MAX;
    if multiply {
        let v = colada_engine::math::U256::from_limbs_le([m, m, m, top]);
        let h = 2 * u128::from(top) + 1;
        let actual = v.checked_mul_u64(2);
        if h > u128::from(m) {
            assert!(matches!(actual, Err(colada_engine::math::MathError::U256Overflow)));
        } else {
            assert_eq!(actual.unwrap(), colada_engine::math::U256::from_limbs_le([m - 1, m, m, h as u64]));
        }
    } else {
        let v = colada_engine::math::U256::from_limbs_le([lo, m, m, top]);
        let s = u128::from(lo) + u128::from(step);
        let carry = (s >> 64) != 0;
        let actual = v.checked_add(colada_engine::math::U256::from_u64(u64::from(step)));
        if carry && top == m {
            assert!(matches!(actual, Err(colada_engine::math::MathError::U256Overflow)));
        } else {
            assert_eq!(actual.unwrap(), colada_engine::math::U256::from_limbs_le([
                s as u64, if carry { 0 } else { m }, if carry { 0 } else { m },
                if carry { top + 1 } else { top },
            ]));
        }
    }
}

#[kani::proof]
#[kani::unwind(194)]
fn u256_top_limb_division_ceil_and_zero_denominator() {
    let k: u8 = kani::any();
    let tail: u8 = kani::any();
    let zero: bool = kani::any();
    // Witnesses: k0,tail0 -> 2^64; tail1 -> 2^64+1; zero -> ZeroDenominator.
    kani::assume(k <= 7);
    let numerator = colada_engine::math::U256::from_limbs_le([u64::from(tail), 0, u64::from(k), 1]);
    let denominator = colada_engine::math::U256::from_limbs_le(if zero { [0; 4] } else { [0, 0, 1, 0] });
    let actual = numerator.div_ceil_bounded(denominator);
    if zero {
        assert!(matches!(actual, Err(colada_engine::math::MathError::ZeroDenominator)));
    } else {
        let out = actual.unwrap();
        assert_eq!(out, colada_engine::math::U256::from_limbs_le([u64::from(k) + u64::from(tail != 0), 1, 0, 0]));
        assert!(matches!(out.try_u64(), Err(colada_engine::math::MathError::InvalidNarrowing)));
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn q64_narrowing_boundary_floor_ceil_split() {
    let t: u16 = kani::any();
    // Witnesses: t0 rejects floor/ceil; t1 floor=MAX, ceil rejects.
    // Product identity: (2^96-t)(2^96+t)=2^192-t^2, with 0<t^2<2^64 for t!=0.
    let b = 1_u128 << 96;
    let left = b - u128::from(t);
    let right = b + u128::from(t);
    let floor = colada_engine::math::mul_q64_floor(left, right);
    if t == 0 {
        assert!(matches!(floor, Err(colada_engine::math::MathError::InvalidNarrowing)));
    } else {
        assert_eq!(floor.unwrap(), u128::MAX);
    }
    assert!(matches!(colada_engine::math::mul_q64_ceil(left, right), Err(colada_engine::math::MathError::InvalidNarrowing)));
}

#[kani::proof]
#[kani::unwind(2)]
fn funded_mixed_price_and_half_index_signed_floor() {
    let j: u8 = kani::any();
    let whole: u8 = kani::any();
    let shift: i8 = kani::any();
    let allocation: u8 = kani::any();
    let case: u8 = kani::any();
    let long: bool = kani::any();
    let up: bool = kani::any();
    let positive_delta: bool = kani::any();
    let credit: bool = kani::any();
    // Witnesses: j0,whole0,long/up/positive_delta=true -> raw-1;
    // j0,long=true,positive_delta=false,credit=true -> receiver0 (whole0), receiver1 (whole1).
    kani::assume(j <= 15 && whole <= 3);
    kani::assume(-4 <= shift && shift <= 4);
    kani::assume(case <= 4);
    let n = 2 * u64::from(j) + 1;
    let size = if long { n as i64 } else { -(n as i64) };
    let q_signed = 1_i128 << 64;
    let cp = i128::from(shift) * q_signed;
    let delta = i128::from(whole) * q_signed + q_signed / 2;
    let cur = if positive_delta { cp + delta } else { cp - delta };
    let size = match case {
        1 => 0,
        2 => if long { (1_i64 << 44) + 1 } else { -((1_i64 << 44) + 1) },
        _ => size,
    };
    let entry = if case == 3 { 0 } else { 2 };
    let exit = if case == 4 { 0 } else if up { 3 } else { 1 };
    let actual = colada_engine::math::funded_net_and_capped(size, entry, exit, cp, cur, u64::from(allocation), credit);
    if case != 0 {
        assert!(matches!(actual, Err(colada_engine::math::MathError::OutOfEnvelope)));
    } else {
        let price = if long == up { i128::from(j) } else { -(i128::from(j) + 1) };
        let payer = long == positive_delta;
        let fmag = i128::from(n) * i128::from(whole) + i128::from(j);
        let funding = if payer { -(fmag + 1) } else { fmag };
        let expected = price + if credit || payer { funding } else { 0 };
        let (raw, capped) = actual.unwrap();
        assert_eq!(raw, expected);
        let lower = -i128::from(allocation);
        if expected < lower {
            assert_eq!(capped, lower);
        } else if expected > i128::from(n) {
            assert_eq!(capped, i128::from(n));
        } else {
            assert_eq!(capped, expected);
        }
    }
}

// Base100,65535; fraction0..255 or65535; short seconds1,16; both axes, all targets.
macro_rules! secure_reference_nonzero_elapsed_rate_and_step_cap_case {
    ($seconds:expr, $unix_limited:expr, $capped:expr, $frac:expr, $base:expr, $target:expr) => {{
        let base: u16 = $base;
        let frac: u16 = $frac;
        let s0: u16 = kani::any();
        let t0: i16 = kani::any();
        let seconds: u8 = $seconds;
        let slack: u8 = kani::any();
        let target: u8 = $target;
        let unix_limited: bool = $unix_limited;
        let capped: bool = $capped;
        let open: bool = kani::any();
        let lifted: bool = kani::any();
        // Witnesses: base100,seconds1,short/up moves on either limiting clock;
        // capped,slack0 binds1%; target2 snaps to base*R.
        kani::assume(base >= 100);
        kani::assume(1 <= seconds && seconds <= 16);
        kani::assume(target <= 2);
        let r = colada_engine::math::SecureReferenceVNext {
            ref_q32: u128::from(base) * colada_engine::math::SECURE_ONE_Q32 + u128::from(frac),
            slot: u64::from(s0), unix: i64::from(t0), lifted,
        };
        let du = if capped { 1800 + u64::from(slack) } else { u64::from(seconds) };
        let ds = if capped { 4500 + 3 * u64::from(slack) } else if unix_limited { 5 * du } else { du };
        let effective_ms = if unix_limited { 1000 * u64::from(seconds) } else { 400 * u64::from(seconds) };
        let nextslot = r.slot + ds;
        let nextunix = r.unix + du as i64;
        let p = match target { 0 => u64::from(base) / 2, 1 => 2 * u64::from(base), _ => u64::from(base) };
        let mode = if open { colada_engine::math::SecureStepMode::Open } else { colada_engine::math::SecureStepMode::Fresh };
        let out = colada_engine::math::secure_reference_step(r, p, nextslot, nextunix, mode);
        assert_eq!(out.lifted, lifted);
        assert_eq!(out.slot, nextslot);
        assert_eq!(out.unix, nextunix);
        assert!(out.ref_q32 <= colada_engine::math::SECURE_P_MAX_Q32);
        if target == 2 {
            assert_eq!(out.ref_q32, u128::from(base) * colada_engine::math::SECURE_ONE_Q32);
        } else {
            if target == 0 { assert!(out.ref_q32 < r.ref_q32); } else { assert!(out.ref_q32 > r.ref_q32); }
            let m = out.ref_q32.abs_diff(r.ref_q32);
            if capped {
                assert!(m * 100 <= r.ref_q32);
                assert!(r.ref_q32 < (m + 1) * 100);
            } else {
                assert!(m * 180_000_000 <= r.ref_q32 * u128::from(effective_ms));
                assert!(r.ref_q32 * u128::from(effective_ms) < (m + 1) * 180_000_000);
            }
            assert!(m * 100 <= r.ref_q32);
        }
    }};
}

macro_rules! secure_reference_nonzero_elapsed_rate_and_step_cap_family {
    ($( $(#[$attr:meta])* fn $name:ident($seconds:expr, $unix_limited:expr, $capped:expr, $frac:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 6);
                match case_id {
                    0 => secure_reference_nonzero_elapsed_rate_and_step_cap_case!($seconds, $unix_limited, $capped, $frac, 100, 0),
                    1 => secure_reference_nonzero_elapsed_rate_and_step_cap_case!($seconds, $unix_limited, $capped, $frac, 100, 1),
                    2 => secure_reference_nonzero_elapsed_rate_and_step_cap_case!($seconds, $unix_limited, $capped, $frac, 100, 2),
                    3 => secure_reference_nonzero_elapsed_rate_and_step_cap_case!($seconds, $unix_limited, $capped, $frac, 65535, 0),
                    4 => secure_reference_nonzero_elapsed_rate_and_step_cap_case!($seconds, $unix_limited, $capped, $frac, 65535, 1),
                    5 => secure_reference_nonzero_elapsed_rate_and_step_cap_case!($seconds, $unix_limited, $capped, $frac, 65535, 2),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: secure_reference_nonzero_elapsed_rate_and_step_cap — instances: s1,s16 × unix,slot plus cap; frac8,fracmax; base100,65535 × targets0,1,2 per proof (10)
secure_reference_nonzero_elapsed_rate_and_step_cap_family! {
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s1_unix_frac8(1, true, false, u16::from(kani::any::<u8>()));
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s1_unix_fracmax(1, true, false, 65535);
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s1_slot_frac8(1, false, false, u16::from(kani::any::<u8>()));
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s1_slot_fracmax(1, false, false, 65535);
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s16_unix_frac8(16, true, false, u16::from(kani::any::<u8>()));
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s16_unix_fracmax(16, true, false, 65535);
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s16_slot_frac8(16, false, false, u16::from(kani::any::<u8>()));
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_s16_slot_fracmax(16, false, false, 65535);
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_capped_frac8(1, false, true, u16::from(kani::any::<u8>()));
    #[kani::proof]
    #[kani::unwind(2)]
    fn secure_reference_nonzero_elapsed_rate_and_step_cap_capped_fracmax(1, false, true, 65535);
}

macro_rules! secure_fill_checked_directional_rounding_bounds_case {
    ($p:expr, $r:expr, $frac:expr, $buy:expr, $x:expr) => {{
        let x: u8 = $x;
        let p: u8 = $p;
        let r: u8 = $r;
        let frac: u16 = $frac;
        let buy: bool = $buy;
        // Witnesses: x7,p3,r4,frac0 -> buy10; x7,p4,r3 -> sell3; x1,p2,r1 -> sell floor1.
        kani::assume(x >= 1 && p >= 1 && r >= 1);
        let reference = u128::from(r) * colada_engine::math::SECURE_ONE_Q32 + u128::from(frac);
        let pq = u128::from(p) * colada_engine::math::SECURE_ONE_Q32;
        let s = if buy { pq.max(reference) } else { pq.min(reference) };
        let a = u128::from(x);
        let b = u128::from(p);
        let f = colada_engine::math::checked_secure_fill(u64::from(x), u64::from(p), reference, buy).unwrap();
        assert_eq!(colada_engine::math::secure_fill(u64::from(x), u64::from(p), reference, buy), f);
        let value = u128::from(f);
        let d = b * colada_engine::math::SECURE_ONE_Q32;
        let n = s * a;
        if buy {
            assert!(f >= u64::from(x));
            assert!(f >= u64::from(p));
            assert!(value * colada_engine::math::SECURE_ONE_Q32 >= s && value * d >= n);
            assert!((value - 1) * colada_engine::math::SECURE_ONE_Q32 < s || (value - 1) * d < n);
        } else {
            assert!(1 <= f && f <= u64::from(x) && f <= u64::from(p));
            if value > 1 {
                assert!(value * colada_engine::math::SECURE_ONE_Q32 <= s && value * d <= n);
            }
            assert!((value + 1) * colada_engine::math::SECURE_ONE_Q32 > s || (value + 1) * d > n);
        }
    }};
}

macro_rules! secure_fill_checked_directional_rounding_bounds_family {
    ($( $(#[$attr:meta])* fn $name:ident($p:expr, $r:expr, $frac:expr, $buy:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 3);
                match case_id {
                    0 => secure_fill_checked_directional_rounding_bounds_case!($p, $r, $frac, $buy, 1),
                    1 => secure_fill_checked_directional_rounding_bounds_case!($p, $r, $frac, $buy, 7),
                    2 => secure_fill_checked_directional_rounding_bounds_case!($p, $r, $frac, $buy, 255),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: secure_fill_checked_directional_rounding_bounds — instances: 8 explicit (p,r,fraction) fixtures × buy,sell; x1,7,255 per proof (16)
secure_fill_checked_directional_rounding_bounds_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p1_r1_f0_buy(1, 1, 0, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p1_r1_f0_sell(1, 1, 0, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p1_r255_fmax_buy(1, 255, 65535, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p1_r255_fmax_sell(1, 255, 65535, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p2_r1_fmax_buy(2, 1, 65535, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p2_r1_fmax_sell(2, 1, 65535, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p3_r4_f0_buy(3, 4, 0, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p3_r4_f0_sell(3, 4, 0, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p3_r4_f1_buy(3, 4, 1, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p3_r4_f1_sell(3, 4, 1, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p4_r3_f0_buy(4, 3, 0, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p4_r3_f0_sell(4, 3, 0, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p4_r3_fmax_buy(4, 3, 65535, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p4_r3_fmax_sell(4, 3, 65535, false);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p255_r255_fmax_buy(255, 255, 65535, true);
    #[kani::proof]
    #[kani::unwind(33)]
    fn secure_fill_checked_directional_rounding_bounds_p255_r255_fmax_sell(255, 255, 65535, false);
}

macro_rules! confidence_rounding_matches_symmetric_breaker_band_case {
    ($k:expr, $deviation:expr) => {{
        let k: u8 = $k;
        let deviation: u16 = $deviation;
        let case: u8 = kani::any();
        let lower_side: bool = kani::any();
        let buy: bool = kani::any();
        // Witnesses: k1,dev1 -> c500/allowed; dev2 -> c1000/rejected; dev0 -> c0.
        kani::assume(1 <= k && k <= 32);
        kani::assume(case <= 3);
        let base = 20 * u64::from(k);
        kani::assume(u64::from(deviation) < base);
        let q = 1_u128 << 64;
        let point = u128::from(base) * q;
        if case == 0 {
            let dev = u64::from(deviation);
            let lo = u128::from(base - dev) * q;
            let hi = u128::from(base + dev) * q;
            let p = if lower_side { base - dev } else { base + dev };
            let reference = u128::from(base) * colada_engine::math::SECURE_ONE_Q32;
            let c = colada_engine::math::compose_confidence_bps(point, lo, hi).unwrap();
            assert!(u64::from(c) * base >= 10000 * dev);
            assert!(c == 0 || (u64::from(c) - 1) * base < 10000 * dev);
            assert!(c <= 10000);
            let allows = colada_engine::math::secure_breaker_allows_open(p, reference, buy);
            assert_eq!(allows, u128::from(c) <= colada_engine::math::SECURE_BREAKER_BPS);
            assert_eq!(allows, 10000 * dev <= 500 * base);
        } else if case == 1 {
            assert!(matches!(colada_engine::math::compose_confidence_bps(0, point, point), Err(colada_engine::math::MathError::InvertedPriceBounds)));
        } else if case == 2 {
            assert!(matches!(colada_engine::math::compose_confidence_bps(point, point + q, point + q), Err(colada_engine::math::MathError::InvertedPriceBounds)));
        } else {
            assert!(matches!(colada_engine::math::compose_confidence_bps(point, point, 8 * point), Err(colada_engine::math::MathError::OutOfEnvelope)));
        }
        kani::cover!(case == 0);
    }};
}

macro_rules! confidence_rounding_matches_symmetric_breaker_band_family {
    ($( $(#[$attr:meta])* fn $name:ident($k:expr); )*) => {
        $(
            $(#[$attr])*
            fn $name() {
                let case_id: u8 = kani::any();
                kani::assume(case_id < 5);
                match case_id {
                    0 => confidence_rounding_matches_symmetric_breaker_band_case!($k, 0),
                    1 => confidence_rounding_matches_symmetric_breaker_band_case!($k, 1),
                    2 => confidence_rounding_matches_symmetric_breaker_band_case!($k, $k),
                    3 => confidence_rounding_matches_symmetric_breaker_band_case!($k, $k + 1),
                    4 => confidence_rounding_matches_symmetric_breaker_band_case!($k, 20 * $k - 1),
                    _ => unreachable!(),
                }
                kani::cover!(case_id == 1);
            }
        )*
    };
}

// Family: confidence_rounding_matches_symmetric_breaker_band — instances: k1,k2,k32; deviation0,1,k,k+1,20k-1 (3)
confidence_rounding_matches_symmetric_breaker_band_family! {
    #[kani::proof]
    #[kani::unwind(33)]
    fn confidence_rounding_matches_symmetric_breaker_band_k1(1);
    #[kani::proof]
    #[kani::unwind(33)]
    fn confidence_rounding_matches_symmetric_breaker_band_k2(2);
    #[kani::proof]
    #[kani::unwind(33)]
    fn confidence_rounding_matches_symmetric_breaker_band_k32(32);
}
