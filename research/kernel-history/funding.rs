#[inline(always)]
pub fn div_i128_floor(numerator: i128, denominator: i128) -> Result<i128, ProgramError> {
    let quotient = numerator
        .checked_div(denominator)
        .ok_or(ProgramError::from(ColadaError::Overflow))?;
    let remainder = numerator
        .checked_rem(denominator)
        .ok_or(ProgramError::from(ColadaError::Overflow))?;
    if remainder != 0 && ((remainder > 0) != (denominator > 0)) {
        quotient
            .checked_sub(1)
            .ok_or(ProgramError::from(ColadaError::Overflow))
    } else {
        Ok(quotient)
    }
}

pub const FUNDING_RATE_UNIT_SLOTS: u64 = 9_000;

const Q32_ONE_I64: i64 = 1i64 << 32;

const Q64_ONE_I128: i128 = 1i128 << 64;

const FUNDING_DEADBAND_Q32: i64 = (Q32_ONE_I64 / 10_000) * 5;

const MAX_FUNDING_PREMIUM_Q32: i64 = (Q32_ONE_I64 / 10_000) * 300;

#[inline(always)]
fn clamp_funding_premium_q32(premium_q32: i64) -> i64 {
    let deadband_abs = if premium_q32 < 0 {
        premium_q32.saturating_abs()
    } else {
        premium_q32
    };
    if deadband_abs < FUNDING_DEADBAND_Q32 {
        return 0;
    }
    premium_q32.clamp(-MAX_FUNDING_PREMIUM_Q32, MAX_FUNDING_PREMIUM_Q32)
}

const FUNDING_RATE_CAP_Q64_64: i128 = (Q64_ONE_I128 / 10_000) * 10;

#[inline(always)]
pub fn div_i128_trunc_toward_zero(
    numerator: i128,
    denominator: i128,
) -> Result<i128, ProgramError> {
    numerator
        .checked_div(denominator)
        .ok_or(ProgramError::from(ColadaError::Overflow))
}

#[inline(always)]
pub fn price_pnl_for_quote_notional(
    position_size: i32,
    close_abs_size: u64,
    entry_price: u64,
    execution_price: u64,
) -> Result<i64, ProgramError> {
    if position_size == 0 || close_abs_size == 0 || entry_price == 0 || execution_price == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let signed_close_size = if position_size > 0 {
        i128::from(close_abs_size)
    } else {
        -i128::from(close_abs_size)
    };
    let price_diff = i128::from(execution_price)
        .checked_sub(i128::from(entry_price))
        .ok_or(ProgramError::from(ColadaError::Overflow))?;
    let pnl_numerator = price_diff
        .checked_mul(signed_close_size)
        .ok_or(ProgramError::from(ColadaError::Overflow))?;
    let pnl = div_i128_floor(pnl_numerator, i128::from(entry_price))?;
    i64::try_from(pnl).map_err(|_| ProgramError::from(ColadaError::Overflow))
}

#[inline(always)]
fn funding_rate_q64_64_from_premium_q32(premium_q32: i64) -> Result<i128, ProgramError> {
    i128::from(premium_q32)
        .checked_shl(32)
        .ok_or(ProgramError::from(ColadaError::Overflow))?
        .checked_div(8)
        .ok_or(ProgramError::from(ColadaError::Overflow))
        .map(|rate| rate.clamp(-FUNDING_RATE_CAP_Q64_64, FUNDING_RATE_CAP_Q64_64))
}