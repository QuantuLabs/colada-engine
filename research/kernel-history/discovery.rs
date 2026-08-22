pub const MAX_POSITION_NOTIONAL_QUOTE_E6: u64 = 1 << 44;

#[inline(always)]
fn cap_v2_robust_notional(
    attacker_set_econ_loss_floor_native: u128,
    max_oracle_deviation_bps: u16,
    cap_safety_factor: u16,
) -> Result<u128, ProgramError> {
    if attacker_set_econ_loss_floor_native == 0
        || max_oracle_deviation_bps == 0
        || cap_safety_factor == 0
    {
        return Err(MatcherError::InvalidOraclePrice.into());
    }
    let band_scaled = attacker_set_econ_loss_floor_native
        .checked_mul(10_000)
        .ok_or(MatcherError::Overflow)?
        .checked_div(u128::from(max_oracle_deviation_bps))
        .ok_or(MatcherError::Overflow)?;
    band_scaled
        .checked_div(u128::from(cap_safety_factor))
        .ok_or(MatcherError::Overflow.into())
}

fn cap_from_floor(floor: u128, deviation: u16, safety: u16) -> Result<u128, ProgramError> {
    if floor == 0 || deviation == 0 || safety == 0 {
        return Err(invalid());
    }
    floor
        .checked_mul(10_000)
        .and_then(|value| value.checked_div(u128::from(deviation)))
        .and_then(|value| value.checked_div(u128::from(safety)))
        .ok_or(ProgramError::ArithmeticOverflow)
}