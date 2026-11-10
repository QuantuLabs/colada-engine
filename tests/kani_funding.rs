#![cfg(kani)]

use colada_engine::funding::*;

// Domain: elapsed u16, initial slot u32, signed rate i16, zero initial carry.
#[kani::proof]
#[kani::unwind(2)]
fn funding_slot_advancement() {
    let elapsed = u64::from(kani::any::<u16>());
    let slot = u64::from(kani::any::<u32>());
    let step =
        funding_index_step_q64_64(0, 0, slot, elapsed, i128::from(kani::any::<i16>())).unwrap();
    assert_eq!(step.last_funding_accrual_slot, slot + elapsed);
    assert_eq!(step.changed, elapsed != 0);
    kani::cover!(step.changed);
}

// Domain: signed size i16, premium i32, elapsed 65535 slots, zero carry.
#[kani::proof]
#[kani::unwind(2)]
fn funding_long_idle_pnl_bounded() {
    let size = i64::from(kani::any::<i16>());
    let rate = funding_rate_q64_64_from_premium_q32(i64::from(kani::any::<i32>())).unwrap();
    let step = funding_index_step_q64_64(0, 0, 0, 65535, rate).unwrap();
    let (pnl, _) =
        funding_pnl_for_signed_size_values(size, 0, step.cumulative_funding_index_q64_64, 0)
            .unwrap();
    // Cap is below 0.001 per 9000 slots; elapsed is below eight periods.
    assert!(i128::from(pnl).abs() <= i128::from(size).abs() / 125 + 1);
    kani::cover!(size != 0 && pnl != 0);
}

// Domain: premium i32 plus concrete signed i64 extrema.
#[kani::proof]
#[kani::unwind(2)]
fn funding_rate_deadband_and_cap() {
    let p = i64::from(kani::any::<i32>());
    let rate = funding_rate_q64_64_from_premium_q32(p).unwrap();
    let cap = ((1_i128 << 64) / 10000) * 10;
    assert!(rate >= -cap && rate <= cap);
    assert!(p == 0 || rate == 0 || (rate > 0) == (p > 0));
    if p.abs() < ((1_i64 << 32) / 10000) * 5 {
        assert_eq!(rate, 0);
    }
    assert_eq!(funding_rate_q64_64_from_premium_q32(i64::MAX), Ok(cap));
    assert_eq!(funding_rate_q64_64_from_premium_q32(i64::MIN), Ok(-cap));
    kani::cover!(rate == cap);
}

// Family: index step reconstructs its numerator — instances: elapsed 1, 2, 256, 9000.
macro_rules! carry_reconstructs_numerator {
    ($( $(#[$attr:meta])* fn $name:ident($slots:expr); )*) => { $(
        $(#[$attr])*
        fn $name() {
            let rate = i128::from(kani::any::<i16>());
            let carry = i128::from(kani::any::<i16>());
            let t: u64 = $slots;
            let s = funding_index_step_q64_64(0, carry, 0, t, rate).unwrap();
            assert_eq!(
                s.cumulative_funding_index_q64_64 * 9000 + s.funding_index_pre_division_remainder,
                rate * i128::from(t) + carry
            );
            assert!(s.funding_index_pre_division_remainder.abs() < 9000);
            kani::cover!(s.changed);
        }
    )* };
}
carry_reconstructs_numerator! {
    // Domain: signed rate i16, carry i16, concrete elapsed slots.
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_carry_reconstructs_numerator_t1(1);
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_carry_reconstructs_numerator_t2(2);
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_carry_reconstructs_numerator_t256(256);
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_carry_reconstructs_numerator_t9000(9000);
}

// Domain: arbitrary i128 index/carry/rate and u64 slot; zero elapsed.
#[kani::proof]
#[kani::unwind(2)]
fn funding_index_zero_elapsed_identity() {
    let index = kani::any();
    let carry = kani::any();
    let rate = kani::any();
    let slot = kani::any();
    let s = funding_index_step_q64_64(index, carry, slot, 0, rate).unwrap();
    assert_eq!(s.cumulative_funding_index_q64_64, index);
    assert_eq!(s.funding_index_pre_division_remainder, carry);
    assert_eq!(s.funding_rate_q64_64_per_9000_slots, rate);
    assert_eq!(s.last_funding_accrual_slot, slot);
    assert!(!s.changed);
    kani::cover!(!s.changed);
}

// Domain: signed size i8, index delta i16 times 2^56, arbitrary u64 carry.
#[kani::proof]
#[kani::unwind(2)]
fn funding_pnl_floor_remainder_canonical() {
    let size = i64::from(kani::any::<i8>());
    let delta = i128::from(kani::any::<i16>()) * (1_i128 << 56);
    let carry = kani::any::<u64>();
    let (pnl, rem) = funding_pnl_for_signed_size_values(size, 0, delta, carry).unwrap();
    let n = -i128::from(size) * delta + i128::from(carry);
    let q = 1_i128 << 64;
    assert_eq!(i128::from(pnl) * q + i128::from(rem), n);
    assert!(i128::from(pnl) * q <= n && n < (i128::from(pnl) + 1) * q);
    kani::cover!(pnl != 0);
}

// Domain: size 1..=256, execution 1..=256, entry 100, either direction.
#[kani::proof]
#[kani::unwind(2)]
fn price_pnl_one_directional_rounding() {
    let size = u64::from(kani::any::<u8>()) + 1;
    let exit = u64::from(kani::any::<u8>()) + 1;
    let sign = if kani::any::<bool>() { 1 } else { -1 };
    let pnl = price_pnl_for_quote_notional(sign, size, 100, exit).unwrap();
    let n = (i128::from(exit) - 100) * i128::from(size) * i128::from(sign);
    assert!(i128::from(pnl) * 100 <= n && n < (i128::from(pnl) + 1) * 100);
    kani::cover!(pnl < 0);
}

// Family: split accrual equals single-step accrual — instances: (1,1), (1,255), (256,256), (4500,4500).
macro_rules! partitioned_accrual {
    ($( $(#[$attr:meta])* fn $name:ident($a:expr, $b:expr); )*) => { $(
        $(#[$attr])*
        fn $name() {
            let rate = i128::from(kani::any::<i16>());
            let (a, b): (u64, u64) = ($a, $b);
            let first = funding_index_step_q64_64(0, 0, 0, a, rate).unwrap();
            let split = funding_index_step_q64_64(
                first.cumulative_funding_index_q64_64,
                first.funding_index_pre_division_remainder,
                a,
                b,
                rate,
            )
            .unwrap();
            let single = funding_index_step_q64_64(0, 0, 0, a + b, rate).unwrap();
            assert_eq!(split, single);
            kani::cover!(single.cumulative_funding_index_q64_64 > 0);
        }
    )* };
}
partitioned_accrual! {
    // Domain: signed rate i16, zero initial carry, concrete elapsed pair.
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_partitioned_accrual_matches_single_step_1_1(1, 1);
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_partitioned_accrual_matches_single_step_1_255(1, 255);
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_partitioned_accrual_matches_single_step_256_256(256, 256);
    #[kani::proof]
    #[kani::unwind(2)]
    fn funding_index_partitioned_accrual_matches_single_step_4500_4500(4500, 4500);
}
