use colada_engine::funding::*;
const Q: i128 = 1_i128 << 64;

#[test]
fn div_i128_floor_rounds_positive_down() {
    assert_eq!(div_i128_floor(10, 3).unwrap(), 3);
}

#[test]
fn div_i128_floor_rounds_negative_toward_solvency() {
    assert_eq!(div_i128_floor(-10, 3).unwrap(), -4);
}

#[test]
fn price_pnl_for_quote_notional_uses_one_directional_basis() {
    assert_eq!(
        price_pnl_for_quote_notional(100, 100, 1_000, 1_100).unwrap(),
        10
    );
    assert_eq!(
        price_pnl_for_quote_notional(-100, 100, 1_000, 900).unwrap(),
        10
    );
    assert_eq!(price_pnl_for_quote_notional(3, 3, 1_000, 995).unwrap(), -1);
    assert_eq!(
        price_pnl_for_quote_notional(10_000_000_000_000, 10_000_000_000_000, 1, 1_000_001),
        Ok(i64::MAX)
    );
    assert_eq!(
        price_pnl_for_quote_notional(9_223_372, 9_223_372, 1, 1_000_000_000_001),
        Ok(9_223_372_000_000_000_000)
    );
    assert_eq!(
        price_pnl_for_quote_notional(-10_000_000_000_000, 10_000_000_000_000, 1_000_001, 1),
        Ok(9_999_990_000_009)
    );
}

#[test]
fn funding_pnl_vectors() {
    assert_eq!(funding_pnl_for_signed_size_values(0, 0, 0, 0), Ok((0, 0)));
    assert_eq!(
        funding_pnl_for_signed_size_values(
            0,
            -170141183460469231731687303715884105728,
            170141183460469231731687303715884105727,
            18446744073709551615
        ),
        Ok((0, 18446744073709551615))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(1, 0, 18446744073709551616, 0),
        Ok((-1, 0))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(-1, 0, 18446744073709551616, 0),
        Ok((1, 0))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(1, 18446744073709551616, 0, 0),
        Ok((1, 0))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(-1, 18446744073709551616, 0, 0),
        Ok((-1, 0))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(1, 0, 9223372036854775808, 0),
        Ok((-1, 9223372036854775808))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(-1, 0, 9223372036854775808, 0),
        Ok((0, 9223372036854775808))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(1, 0, 0, 18446744073709551615),
        Ok((0, 18446744073709551615))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(-1, 0, 0, 18446744073709551615),
        Ok((0, 18446744073709551615))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(
            9223372036854775807,
            0,
            170141183460469231731687303715884105727,
            0
        ),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(
            -9223372036854775808,
            0,
            170141183460469231731687303715884105727,
            0
        ),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(
            1,
            -170141183460469231731687303715884105728,
            170141183460469231731687303715884105727,
            0
        ),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(1, 0, -170141183460469231731687303715884105728, 0),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(
            9223372036854775807,
            0,
            18446744073709551616,
            18446744073709551615
        ),
        Ok((-9223372036854775807, 18446744073709551615))
    );
    assert_eq!(
        funding_pnl_for_signed_size_values(
            -9223372036854775808,
            0,
            18446744073709551616,
            18446744073709551615
        ),
        Err(FundingError::Overflow)
    );
}

#[test]
fn funding_rate_edges() {
    assert_eq!(clamp_funding_premium_q32(-9223372036854775808), -128848800);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(-9223372036854775808),
        Ok(-18446744073709550)
    );
    assert_eq!(clamp_funding_premium_q32(-128848801), -128848800);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(-128848801),
        Ok(-18446744073709550)
    );
    assert_eq!(clamp_funding_premium_q32(-128848800), -128848800);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(-128848800),
        Ok(-18446744073709550)
    );
    assert_eq!(clamp_funding_premium_q32(-2147480), -2147480);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(-2147480),
        Ok(-1152919546101760)
    );
    assert_eq!(clamp_funding_premium_q32(-2147479), 0);
    assert_eq!(funding_rate_q64_64_from_premium_q32(-2147479), Ok(0));
    assert_eq!(clamp_funding_premium_q32(0), 0);
    assert_eq!(funding_rate_q64_64_from_premium_q32(0), Ok(0));
    assert_eq!(clamp_funding_premium_q32(2147479), 0);
    assert_eq!(funding_rate_q64_64_from_premium_q32(2147479), Ok(0));
    assert_eq!(clamp_funding_premium_q32(2147480), 2147480);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(2147480),
        Ok(1152919546101760)
    );
    assert_eq!(clamp_funding_premium_q32(128848800), 128848800);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(128848800),
        Ok(18446744073709550)
    );
    assert_eq!(clamp_funding_premium_q32(128848801), 128848800);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(128848801),
        Ok(18446744073709550)
    );
    assert_eq!(clamp_funding_premium_q32(9223372036854775807), 128848800);
    assert_eq!(
        funding_rate_q64_64_from_premium_q32(9223372036854775807),
        Ok(18446744073709550)
    );
}

#[test]
fn funding_index_vectors() {
    assert_eq!(
        funding_index_step_q64_64(0, 0, 10, 0, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 0,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: 0,
            last_funding_accrual_slot: 10,
            changed: false
        })
    );
    assert_eq!(
        funding_index_step_q64_64(0, 8999, 10, 0, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 0,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: 8999,
            last_funding_accrual_slot: 10,
            changed: false
        })
    );
    assert_eq!(
        funding_index_step_q64_64(0, -8999, 10, 0, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 0,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: -8999,
            last_funding_accrual_slot: 10,
            changed: false
        })
    );
    assert_eq!(
        funding_index_step_q64_64(0, 0, 10, 8999, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 0,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: 0,
            last_funding_accrual_slot: 9009,
            changed: true
        })
    );
    assert_eq!(
        funding_index_step_q64_64(0, 8999, 10, 9000, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 0,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: 8999,
            last_funding_accrual_slot: 9010,
            changed: true
        })
    );
    assert_eq!(
        funding_index_step_q64_64(0, -8999, 10, 9001, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 0,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: -8999,
            last_funding_accrual_slot: 9011,
            changed: true
        })
    );
    assert_eq!(
        funding_index_step_q64_64(17, -8999, 10, 9000, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: 17,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: -8999,
            last_funding_accrual_slot: 9010,
            changed: true
        })
    );
    assert_eq!(
        funding_index_step_q64_64(-17, 8999, 10, 9000, 0),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: -17,
            funding_rate_q64_64_per_9000_slots: 0,
            funding_index_pre_division_remainder: 8999,
            last_funding_accrual_slot: 9010,
            changed: true
        })
    );
    assert_eq!(
        funding_index_step_q64_64(170141183460469231731687303715884105727, 0, 10, 9000, 1),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_index_step_q64_64(-170141183460469231731687303715884105727, 0, 10, 9000, -1),
        Ok(FundingIndexStep {
            cumulative_funding_index_q64_64: -170141183460469231731687303715884105728,
            funding_rate_q64_64_per_9000_slots: -1,
            funding_index_pre_division_remainder: 0,
            last_funding_accrual_slot: 9010,
            changed: true
        })
    );
    assert_eq!(
        funding_index_step_q64_64(0, 0, 18446744073709551615, 1, 0),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_index_step_q64_64(0, 0, 10, 2, 170141183460469231731687303715884105727),
        Err(FundingError::Overflow)
    );
    assert_eq!(
        funding_index_step_q64_64(0, 0, 10, 2, -170141183460469231731687303715884105728),
        Err(FundingError::Overflow)
    );
}

#[test]
fn division_and_input_boundaries() {
    assert_eq!(div_i128_trunc_toward_zero(-10, 3), Ok(-3));
    for (n, d) in [(0, 0), (i128::MIN, -1)] {
        assert_eq!(div_i128_floor(n, d), Err(FundingError::Overflow));
        assert_eq!(
            div_i128_trunc_toward_zero(n, d),
            Err(FundingError::Overflow)
        );
    }
    for args in [(0, 1, 1, 1), (1, 0, 1, 1), (1, 1, 0, 1), (1, 1, 1, 0)] {
        assert_eq!(
            price_pnl_for_quote_notional(args.0, args.1, args.2, args.3),
            Err(FundingError::InvalidInput)
        );
    }
    assert_eq!(
        funding_pnl_for_signed_size_values(1, 0, Q / 2, 0),
        Ok((-1, 1_u64 << 63))
    );
}
