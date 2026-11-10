//! Scalar funding accrual, carry and directional profit/loss arithmetic.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FundingError {
    Overflow,
    InvalidInput,
}

const FUNDING_RATE_UNIT_SLOTS: u64 = 9_000;
const Q32_ONE_I64: i64 = 1i64 << 32;
const Q64_ONE_I128: i128 = 1i128 << 64;
const FUNDING_DEADBAND_Q32: i64 = (Q32_ONE_I64 / 10_000) * 5;
const MAX_FUNDING_PREMIUM_Q32: i64 = (Q32_ONE_I64 / 10_000) * 300;
const FUNDING_RATE_CAP_Q64_64: i128 = (Q64_ONE_I128 / 10_000) * 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FundingIndexStep {
    pub cumulative_funding_index_q64_64: i128,
    pub funding_rate_q64_64_per_9000_slots: i128,
    pub funding_index_pre_division_remainder: i128,
    pub last_funding_accrual_slot: u64,
    pub changed: bool,
}

#[inline(always)]
pub fn clamp_funding_premium_q32(premium_q32: i64) -> i64 {
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

#[inline(always)]
pub fn div_i128_trunc_toward_zero(
    numerator: i128,
    denominator: i128,
) -> Result<i128, FundingError> {
    numerator
        .checked_div(denominator)
        .ok_or(FundingError::Overflow)
}

#[inline(always)]
pub fn funding_rate_q64_64_from_premium_q32(premium_q32: i64) -> Result<i128, FundingError> {
    i128::from(clamp_funding_premium_q32(premium_q32))
        .checked_shl(32)
        .ok_or(FundingError::Overflow)?
        .checked_div(8)
        .ok_or(FundingError::Overflow)
        .map(|rate| rate.clamp(-FUNDING_RATE_CAP_Q64_64, FUNDING_RATE_CAP_Q64_64))
}

#[inline(always)]
pub fn funding_index_step_q64_64(
    cumulative_funding_index_q64_64: i128,
    funding_index_pre_division_remainder: i128,
    last_funding_accrual_slot: u64,
    accrued_slots: u64,
    funding_rate_q64_64_per_9000_slots: i128,
) -> Result<FundingIndexStep, FundingError> {
    if accrued_slots == 0 {
        return Ok(FundingIndexStep {
            cumulative_funding_index_q64_64,
            funding_rate_q64_64_per_9000_slots,
            funding_index_pre_division_remainder,
            last_funding_accrual_slot,
            changed: false,
        });
    }
    let funding_index_numerator = funding_rate_q64_64_per_9000_slots
        .checked_mul(i128::from(accrued_slots))
        .ok_or(FundingError::Overflow)?
        .checked_add(funding_index_pre_division_remainder)
        .ok_or(FundingError::Overflow)?;
    let funding_index_delta =
        div_i128_trunc_toward_zero(funding_index_numerator, i128::from(FUNDING_RATE_UNIT_SLOTS))?;
    let funding_index_remainder = funding_index_numerator
        .checked_sub(
            funding_index_delta
                .checked_mul(i128::from(FUNDING_RATE_UNIT_SLOTS))
                .ok_or(FundingError::Overflow)?,
        )
        .ok_or(FundingError::Overflow)?;
    let cumulative_index = cumulative_funding_index_q64_64
        .checked_add(funding_index_delta)
        .ok_or(FundingError::Overflow)?;
    let last_funding_accrual_slot = last_funding_accrual_slot
        .checked_add(accrued_slots)
        .ok_or(FundingError::Overflow)?;
    Ok(FundingIndexStep {
        cumulative_funding_index_q64_64: cumulative_index,
        funding_rate_q64_64_per_9000_slots,
        funding_index_pre_division_remainder: funding_index_remainder,
        last_funding_accrual_slot,
        changed: true,
    })
}

#[inline(always)]
pub fn funding_pnl_for_signed_size_values(
    signed_size: i64,
    entry_funding_index_q64_64: i128,
    current_funding_index_q64_64: i128,
    carried_remainder_quote_q64_64: u64,
) -> Result<(i64, u64), FundingError> {
    if signed_size == 0 {
        return Ok((0, carried_remainder_quote_q64_64));
    }
    let index_delta = current_funding_index_q64_64
        .checked_sub(entry_funding_index_q64_64)
        .ok_or(FundingError::Overflow)?;
    let funding_pnl_numerator = i128::from(signed_size)
        .checked_mul(index_delta)
        .ok_or(FundingError::Overflow)?
        .checked_neg()
        .ok_or(FundingError::Overflow)?;
    let carried_numerator = funding_pnl_numerator
        .checked_add(i128::from(carried_remainder_quote_q64_64))
        .ok_or(FundingError::Overflow)?;
    let funding_pnl = div_i128_floor(carried_numerator, Q64_ONE_I128)?;
    if funding_pnl > i64::MAX as i128 || funding_pnl < i64::MIN as i128 {
        return Err(FundingError::Overflow);
    }
    let realized_atoms_q64 = funding_pnl
        .checked_mul(Q64_ONE_I128)
        .ok_or(FundingError::Overflow)?;
    let remainder = carried_numerator
        .checked_sub(realized_atoms_q64)
        .ok_or(FundingError::Overflow)?;
    if remainder < 0 || remainder > u64::MAX as i128 {
        return Err(FundingError::Overflow);
    }
    Ok((funding_pnl as i64, remainder as u64))
}

pub fn div_i128_floor(numerator: i128, denominator: i128) -> Result<i128, FundingError> {
    let quotient = numerator
        .checked_div(denominator)
        .ok_or(FundingError::Overflow)?;
    let remainder = numerator
        .checked_rem(denominator)
        .ok_or(FundingError::Overflow)?;
    if remainder != 0 && ((remainder > 0) != (denominator > 0)) {
        quotient.checked_sub(1).ok_or(FundingError::Overflow)
    } else {
        Ok(quotient)
    }
}

#[inline(always)]
pub fn price_pnl_for_quote_notional(
    position_size: i64,
    close_abs_size: u64,
    entry_price: u64,
    execution_price: u64,
) -> Result<i64, FundingError> {
    if position_size == 0 || close_abs_size == 0 || entry_price == 0 || execution_price == 0 {
        return Err(FundingError::InvalidInput);
    }
    let signed_close_size = if position_size > 0 {
        i128::from(close_abs_size)
    } else {
        -i128::from(close_abs_size)
    };
    let price_diff = i128::from(execution_price)
        .checked_sub(i128::from(entry_price))
        .ok_or(FundingError::Overflow)?;
    let pnl_numerator = price_diff
        .checked_mul(signed_close_size)
        .ok_or(FundingError::Overflow)?;
    let pnl = div_i128_floor(pnl_numerator, i128::from(entry_price))?;
    i64::try_from(pnl.min(i128::from(i64::MAX))).map_err(|_| FundingError::Overflow)
}
