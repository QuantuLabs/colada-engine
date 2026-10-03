use core::cmp::Ordering;

use pinocchio::error::ProgramError;

use crate::errors::ColadaError;

pub(crate) type CollateralAtoms = u64;
pub(crate) type QuoteE6 = u64;
pub(crate) type QuoteE6Wide = U256;
pub(crate) type SignedQuoteE6 = i128;
pub(crate) type PriceQ64 = u128;

const Q64_ONE: PriceQ64 = 1_u128 << 64;
const QUOTE_E6_SCALE: u64 = 1_000_000;
const BPS_DENOMINATOR: u64 = 10_000;
const COLLATERAL_FACTOR_BPS: u64 = 5_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MathError {
    InvalidDecimals,
    ZeroPrice,
    InvertedPriceBounds,
    DefaultValueRejected,
    IdentityAddressMismatch,
    OutOfEnvelope,
    U256Overflow,
    U256Underflow,
    ZeroDenominator,
    InvalidNarrowing,
    InsufficientFreeAtoms,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct U256([u64; 4]);

impl U256 {
    const ZERO: Self = Self([0; 4]);
    const ONE: Self = Self([1, 0, 0, 0]);
    const MAX: Self = Self([u64::MAX; 4]);

    pub(crate) const fn from_u64(value: u64) -> Self {
        Self([value, 0, 0, 0])
    }

    pub(super) const fn from_u128(value: u128) -> Self {
        Self([value as u64, (value >> 64) as u64, 0, 0])
    }

    pub(super) const fn from_limbs_le(value: [u64; 4]) -> Self {
        Self(value)
    }

    fn compare(self, rhs: Self) -> Ordering {
        for index in (0..4).rev() {
            match self.0[index].cmp(&rhs.0[index]) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        Ordering::Equal
    }

    pub(super) fn checked_add(self, rhs: Self) -> Result<Self, MathError> {
        let mut out = [0_u64; 4];
        let mut carry = 0_u128;
        for (index, slot) in out.iter_mut().enumerate() {
            let sum = self.0[index] as u128 + rhs.0[index] as u128 + carry;
            *slot = sum as u64;
            carry = sum >> 64;
        }
        if carry != 0 {
            return Err(MathError::U256Overflow);
        }
        Ok(Self(out))
    }

    fn checked_sub(self, rhs: Self) -> Result<Self, MathError> {
        let mut out = [0_u64; 4];
        let mut borrow = 0_u128;
        for (index, slot) in out.iter_mut().enumerate() {
            let left = self.0[index] as u128;
            let right = rhs.0[index] as u128 + borrow;
            if left >= right {
                *slot = (left - right) as u64;
                borrow = 0;
            } else {
                *slot = ((1_u128 << 64) + left - right) as u64;
                borrow = 1;
            }
        }
        if borrow != 0 {
            return Err(MathError::U256Underflow);
        }
        Ok(Self(out))
    }

    pub(super) fn checked_mul_u64(self, rhs: u64) -> Result<Self, MathError> {
        let mut out = [0_u64; 4];
        let mut carry = 0_u128;
        for (index, slot) in out.iter_mut().enumerate() {
            let product = self.0[index] as u128 * rhs as u128 + carry;
            *slot = product as u64;
            carry = product >> 64;
        }
        if carry != 0 {
            return Err(MathError::U256Overflow);
        }
        Ok(Self(out))
    }

    fn checked_mul_u128(self, rhs: u128) -> Result<Self, MathError> {
        let low = self.checked_mul_u64(rhs as u64)?;
        let high = self.checked_mul_u64((rhs >> 64) as u64)?.checked_shl_64()?;
        low.checked_add(high)
    }

    fn checked_shl_64(self) -> Result<Self, MathError> {
        if self.0[3] != 0 {
            return Err(MathError::U256Overflow);
        }
        Ok(Self([0, self.0[0], self.0[1], self.0[2]]))
    }

    fn bit(self, index: usize) -> bool {
        ((self.0[index / 64] >> (index % 64)) & 1) != 0
    }

    fn div_rem_bounded(self, denominator: Self) -> Result<(Self, Self), MathError> {
        if denominator == Self::ZERO {
            return Err(MathError::ZeroDenominator);
        }
        if self.0[2] == 0
            && self.0[3] == 0
            && denominator.0[2] == 0
            && denominator.0[3] == 0
        {
            let numerator = u128::from(self.0[0]) | (u128::from(self.0[1]) << 64);
            let denominator =
                u128::from(denominator.0[0]) | (u128::from(denominator.0[1]) << 64);
            return Ok((
                Self::from_u128(numerator / denominator),
                Self::from_u128(numerator % denominator),
            ));
        }
        let mut quotient = Self::ZERO;
        let mut remainder = Self::ZERO;
        let significant_bits = self
            .0
            .iter()
            .rposition(|limb| *limb != 0)
            .map_or(0, |index| {
                index * 64 + (64 - self.0[index].leading_zeros() as usize)
            });
        for bit in (0..significant_bits).rev() {
            if remainder.0[3] >> 63 != 0 {
                return Err(MathError::U256Overflow);
            }
            let mut shifted = [0_u64; 4];
            let mut carry = 0_u64;
            for (index, slot) in shifted.iter_mut().enumerate() {
                let next = remainder.0[index] >> 63;
                *slot = (remainder.0[index] << 1) | carry;
                carry = next;
            }
            remainder = Self(shifted);
            if self.bit(bit) {
                remainder = remainder.checked_add(Self::ONE)?;
            }
            if remainder.compare(denominator) != Ordering::Less {
                remainder = remainder.checked_sub(denominator)?;
                quotient.0[bit / 64] |= 1_u64 << (bit % 64);
            }
        }
        Ok((quotient, remainder))
    }

    pub(super) fn div_ceil_bounded(self, denominator: Self) -> Result<Self, MathError> {
        let (quotient, remainder) = self.div_rem_bounded(denominator)?;
        if remainder == Self::ZERO {
            Ok(quotient)
        } else {
            quotient.checked_add(Self::ONE)
        }
    }

    pub(crate) fn try_u64(self) -> Result<u64, MathError> {
        if self.0[1] != 0 || self.0[2] != 0 || self.0[3] != 0 {
            return Err(MathError::InvalidNarrowing);
        }
        Ok(self.0[0])
    }
}

pub(crate) fn mul_q64_floor(left: u128, right: u128) -> Result<u128, MathError> {
    if left == 0 || right == 0 {
        return Err(MathError::ZeroPrice);
    }
    let product = U256::from_u128(left).checked_mul_u128(right)?;
    let value = U256([product.0[1], product.0[2], product.0[3], 0]);
    if value.0[2] != 0 || value.0[3] != 0 {
        return Err(MathError::InvalidNarrowing);
    }
    Ok(u128::from(value.0[0]) | (u128::from(value.0[1]) << 64))
}

pub(crate) fn mul_q64_ceil(left: u128, right: u128) -> Result<u128, MathError> {
    if left == 0 || right == 0 {
        return Err(MathError::ZeroPrice);
    }
    let product = U256::from_u128(left).checked_mul_u128(right)?;
    let value = U256([product.0[1], product.0[2], product.0[3], 0]);
    let value = if product.0[0] != 0 {
        value.checked_add(U256::ONE)?
    } else {
        value
    };
    if value.0[2] != 0 || value.0[3] != 0 {
        return Err(MathError::InvalidNarrowing);
    }
    Ok(u128::from(value.0[0]) | (u128::from(value.0[1]) << 64))
}

pub(crate) fn compose_confidence_bps(
    point: u128,
    lower: u128,
    upper: u128,
) -> Result<u16, MathError> {
    if point == 0 || lower == 0 || lower > point || point > upper {
        return Err(MathError::InvertedPriceBounds);
    }
    let deviation = point
        .checked_sub(lower)
        .ok_or(MathError::U256Underflow)?
        .max(upper.checked_sub(point).ok_or(MathError::U256Underflow)?);
    let bps = U256::from_u128(deviation)
        .checked_mul_u64(BPS_DENOMINATOR)?
        .div_ceil_bounded(U256::from_u128(point))?
        .try_u64()?;
    u16::try_from(bps).map_err(|_| MathError::OutOfEnvelope)
}

pub(crate) fn compose_dispersion_bps(left: u16, right: u16) -> Result<u16, MathError> {
    let cross = u64::from(left)
        .checked_mul(u64::from(right))
        .ok_or(MathError::U256Overflow)?
        .checked_add(BPS_DENOMINATOR - 1)
        .ok_or(MathError::U256Overflow)?
        / BPS_DENOMINATOR;
    let value = u64::from(left)
        .checked_add(u64::from(right))
        .and_then(|v| v.checked_add(cross))
        .ok_or(MathError::U256Overflow)?;
    if value > BPS_DENOMINATOR {
        return Err(MathError::OutOfEnvelope);
    }
    Ok(value as u16)
}

pub(crate) fn normalize_sol_atom_cap_quote_e6(
    cap_sol_atoms: u128,
    sol_quote_lower_q64: u128,
) -> Result<u128, MathError> {
    if cap_sol_atoms == 0 || sol_quote_lower_q64 == 0 {
        return Err(MathError::ZeroPrice);
    }
    let numerator = U256::from_u128(cap_sol_atoms)
        .checked_mul_u128(sol_quote_lower_q64)?
        .checked_mul_u64(QUOTE_E6_SCALE)?;
    let denominator = U256::from_u64(1_000_000_000).checked_shl_64()?;
    let value = numerator.div_rem_bounded(denominator)?.0;
    if value.0[2] != 0 || value.0[3] != 0 {
        return Err(MathError::InvalidNarrowing);
    }
    Ok(u128::from(value.0[0]) | (u128::from(value.0[1]) << 64))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PriceBoundsQ64 {
    p_lo: PriceQ64,
    p_hi: PriceQ64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedQuoteCap(QuoteE6);

impl AuthenticatedQuoteCap {
    pub(crate) const fn from_authenticated_parent(value: QuoteE6) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenRequirement {
    pub(crate) required_pre_haircut_quote_e6_wide: QuoteE6Wide,
    pub(crate) required_allocation_atoms: CollateralAtoms,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FinalDebitPartition {
    pub(crate) theoretical_debit_atoms_wide: U256,
    pub(crate) debited_atoms: CollateralAtoms,
    pub(crate) covered_loss_quote_e6: QuoteE6,
    pub(crate) uncovered_loss_quote_e6: QuoteE6,
}

impl From<MathError> for ProgramError {
    fn from(error: MathError) -> Self {
        let error = match error {
            MathError::InvalidDecimals | MathError::DefaultValueRejected => {
                ColadaError::InvalidCollateralProfile
            }
            MathError::IdentityAddressMismatch => ColadaError::CollateralProfileMismatch,
            MathError::ZeroPrice | MathError::InvertedPriceBounds => {
                ColadaError::InvalidOraclePrice
            }
            MathError::OutOfEnvelope
            | MathError::U256Overflow
            | MathError::ZeroDenominator
            | MathError::InvalidNarrowing => ColadaError::Overflow,
            MathError::U256Underflow => ColadaError::Underflow,
            MathError::InsufficientFreeAtoms => ColadaError::InsufficientBalance,
        };
        error.into()
    }
}

fn collateral_scale(collateral_decimals: u8) -> Result<u64, MathError> {
    match collateral_decimals {
        6 => Ok(1_000_000),
        7 => Ok(10_000_000),
        8 => Ok(100_000_000),
        9 => Ok(1_000_000_000),
        _ => Err(MathError::InvalidDecimals),
    }
}

pub(crate) fn checked_price_bounds(
    p_lo: PriceQ64,
    p_hi: PriceQ64,
) -> Result<PriceBoundsQ64, MathError> {
    if p_lo == 0 || p_hi == 0 {
        return Err(MathError::ZeroPrice);
    }
    if p_lo > p_hi {
        return Err(MathError::InvertedPriceBounds);
    }
    Ok(PriceBoundsQ64 { p_lo, p_hi })
}

pub(crate) fn quote_e6_floor_wide(
    atoms: CollateralAtoms,
    price_q64: PriceQ64,
    collateral_decimals: u8,
) -> Result<QuoteE6Wide, MathError> {
    let scale = collateral_scale(collateral_decimals)?;
    if price_q64 == 0 {
        return Err(MathError::ZeroPrice);
    }
    let numerator = U256::from_u64(atoms)
        .checked_mul_u128(price_q64)?
        .checked_mul_u64(QUOTE_E6_SCALE)?;
    let denominator = U256::from_u64(scale).checked_shl_64()?;
    Ok(numerator.div_rem_bounded(denominator)?.0)
}

fn atom_ratio_wide(
    quote_e6_wide: QuoteE6Wide,
    price_q64: PriceQ64,
    collateral_decimals: u8,
) -> Result<(U256, U256), MathError> {
    let scale = collateral_scale(collateral_decimals)?;
    if price_q64 == 0 {
        return Err(MathError::ZeroPrice);
    }
    let numerator = quote_e6_wide
        .checked_mul_u64(scale)
        .and_then(U256::checked_shl_64)
        .map_err(|error| match error {
            MathError::U256Overflow => MathError::OutOfEnvelope,
            other => other,
        })?;
    let denominator = U256::from_u128(price_q64).checked_mul_u64(QUOTE_E6_SCALE)?;
    Ok((numerator, denominator))
}

pub(crate) fn atoms_debit_ceil_wide(
    quote_e6_wide: QuoteE6Wide,
    price_q64: PriceQ64,
    collateral_decimals: u8,
) -> Result<U256, MathError> {
    let (numerator, denominator) = atom_ratio_wide(quote_e6_wide, price_q64, collateral_decimals)?;
    numerator.div_ceil_bounded(denominator)
}

pub(crate) fn atoms_payout_floor_wide(
    quote_e6_wide: QuoteE6Wide,
    price_q64: PriceQ64,
    collateral_decimals: u8,
) -> Result<U256, MathError> {
    let (numerator, denominator) = atom_ratio_wide(quote_e6_wide, price_q64, collateral_decimals)?;
    Ok(numerator.div_rem_bounded(denominator)?.0)
}

pub(crate) fn credited_quote_e6_wide(
    atoms: CollateralAtoms,
    p_lo_q64: PriceQ64,
    collateral_decimals: u8,
) -> Result<QuoteE6Wide, MathError> {
    let raw = quote_e6_floor_wide(atoms, p_lo_q64, collateral_decimals)?;
    let numerator = raw.checked_mul_u64(COLLATERAL_FACTOR_BPS)?;
    Ok(numerator
        .div_rem_bounded(U256::from_u64(BPS_DENOMINATOR))?
        .0)
}

pub(crate) fn cap_quote_wide(
    value: QuoteE6Wide,
    authenticated_cap: AuthenticatedQuoteCap,
) -> QuoteE6 {
    if value.compare(U256::from_u64(authenticated_cap.0)) == Ordering::Greater {
        authenticated_cap.0
    } else {
        value.0[0]
    }
}

pub(crate) fn open_pre_haircut_requirement(
    initial_margin_quote_e6: QuoteE6,
    prices: PriceBoundsQ64,
    collateral_decimals: u8,
    free_atoms: CollateralAtoms,
) -> Result<OpenRequirement, MathError> {
    collateral_scale(collateral_decimals)?;
    let pre_num = U256::from_u64(initial_margin_quote_e6).checked_mul_u64(BPS_DENOMINATOR)?;
    let required_pre_haircut_quote_e6_wide =
        pre_num.div_ceil_bounded(U256::from_u64(COLLATERAL_FACTOR_BPS))?;
    let required_wide = atoms_debit_ceil_wide(
        required_pre_haircut_quote_e6_wide,
        prices.p_lo,
        collateral_decimals,
    )?;
    let required_allocation_atoms = required_wide.try_u64()?;
    if required_allocation_atoms > free_atoms {
        return Err(MathError::InsufficientFreeAtoms);
    }
    Ok(OpenRequirement {
        required_pre_haircut_quote_e6_wide,
        required_allocation_atoms,
    })
}

pub(crate) fn final_debit_partition(
    loss_quote_e6: QuoteE6,
    prices: PriceBoundsQ64,
    collateral_decimals: u8,
    eligible_atoms: CollateralAtoms,
) -> Result<FinalDebitPartition, MathError> {
    collateral_scale(collateral_decimals)?;
    let theoretical_debit_atoms_wide = atoms_debit_ceil_wide(
        U256::from_u64(loss_quote_e6),
        prices.p_lo,
        collateral_decimals,
    )?;
    let eligible_wide = U256::from_u64(eligible_atoms);
    let fully_covered = theoretical_debit_atoms_wide.compare(eligible_wide) != Ordering::Greater;
    let debited_atoms = if fully_covered {
        theoretical_debit_atoms_wide.try_u64()?
    } else {
        eligible_atoms
    };
    let covered_loss_quote_e6 = if fully_covered {
        loss_quote_e6
    } else {
        cap_quote_wide(
            quote_e6_floor_wide(debited_atoms, prices.p_lo, collateral_decimals)?,
            AuthenticatedQuoteCap::from_authenticated_parent(loss_quote_e6),
        )
    };
    let uncovered_loss_quote_e6 = loss_quote_e6
        .checked_sub(covered_loss_quote_e6)
        .ok_or(MathError::U256Underflow)?;
    Ok(FinalDebitPartition {
        theoretical_debit_atoms_wide,
        debited_atoms,
        covered_loss_quote_e6,
        uncovered_loss_quote_e6,
    })
}

pub(crate) fn displayed_subatom_impairment_bound_quote_e6(
    prices: PriceBoundsQ64,
    collateral_decimals: u8,
) -> Result<QuoteE6Wide, MathError> {
    let scale = collateral_scale(collateral_decimals)?;
    let numerator = U256::from_u128(prices.p_hi).checked_mul_u64(QUOTE_E6_SCALE)?;
    let denominator = U256::from_u64(scale).checked_shl_64()?;
    numerator.div_ceil_bounded(denominator)
}

pub(crate) fn identity_price_bounds_by_address_equality(
    collateral_mint: &[u8; 32],
    quote_mint: &[u8; 32],
    authenticated_mint_decimals: u8,
) -> Result<PriceBoundsQ64, MathError> {
    if collateral_mint.iter().all(|byte| *byte == 0) || quote_mint.iter().all(|byte| *byte == 0) {
        return Err(MathError::DefaultValueRejected);
    }
    if collateral_mint != quote_mint {
        return Err(MathError::IdentityAddressMismatch);
    }
    if authenticated_mint_decimals != 6 {
        return Err(MathError::InvalidDecimals);
    }
    Ok(PriceBoundsQ64 {
        p_lo: Q64_ONE,
        p_hi: Q64_ONE,
    })
}

const FUNDED_MAX_ENTRY: u64 = 1 << 44;

fn funded_signed_floor(negative: bool, quotient: u128, remainder: bool) -> i128 {
    if negative {
        -((quotient + u128::from(remainder)) as i128)
    } else {
        quotient as i128
    }
}

pub(crate) fn funded_net_and_capped(
    size: i64,
    entry: u64,
    exit: u64,
    checkpoint: i128,
    current: i128,
    allocated_quote_e6: u64,
    credit_funding: bool,
) -> Result<(i128, i128), MathError> {
    let n = size.unsigned_abs();
    if n == 0 || n > FUNDED_MAX_ENTRY || entry == 0 || exit == 0 {
        return Err(MathError::OutOfEnvelope);
    }
    let price_mag = u128::from(n) * u128::from(exit.abs_diff(entry));
    let price = funded_signed_floor(
        (size < 0) != (exit < entry),
        price_mag / u128::from(entry),
        price_mag % u128::from(entry) != 0,
    );
    let delta = current.abs_diff(checkpoint);
    let low = u128::from(n) * u128::from(delta as u64);
    let funding_q = u128::from(n) * (delta >> 64) + (low >> 64);
    let funding = funded_signed_floor(
        (size > 0) == (current >= checkpoint),
        funding_q,
        low as u64 != 0,
    );
    let raw = price + if credit_funding { funding } else { funding.min(0) };
    Ok((raw, raw.clamp(-i128::from(allocated_quote_e6), i128::from(n))))
}

pub(crate) const SECURE_RATE_BPS_PER_HOUR: u128 = 200;
pub(crate) const SECURE_STEP_CAP_BPS: u128 = 100;
pub(crate) const SECURE_SLOT_MS: u128 = 400;
pub(crate) const SECURE_BREAKER_BPS: u128 = 500;
pub(crate) const SECURE_BREAKER_BOTH_SIDES: bool = true;
pub(crate) const SECURE_IM_FLOOR_BPS: u16 = 5_000;
const HOUR_MS: u128 = 3_600_000;
pub(crate) const SECURE_ONE_Q32: u128 = 1 << 32;
pub(crate) const SECURE_P_MAX_Q32: u128 = (u64::MAX as u128) << 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SecureStepMode {
    Fresh,
    Open,
    Frozen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SecureReferenceVNext {
    pub lifted: bool,
    pub ref_q32: u128,
    pub slot: u64,
    pub unix: i64,
}

pub(crate) fn secure_reference_step(
    r: SecureReferenceVNext,
    p: u64,
    slot: u64,
    unix: i64,
    mode: SecureStepMode,
) -> SecureReferenceVNext {
    secure_reference_step_at(SECURE_RATE_BPS_PER_HOUR, r, p, slot, unix, mode)
}

fn secure_reference_step_at(
    rate_bps_per_hour: u128,
    r: SecureReferenceVNext,
    p: u64,
    slot: u64,
    unix: i64,
    mode: SecureStepMode,
) -> SecureReferenceVNext {
    if mode == SecureStepMode::Frozen {
        return r;
    }
    let du = u128::from(unix.saturating_sub(r.unix).max(0) as u64) * 1000;
    let ds = u128::from(slot.saturating_sub(r.slot)) * SECURE_SLOT_MS;
    let num = (rate_bps_per_hour * du.min(ds)).min(SECURE_STEP_CAP_BPS * HOUR_MS);
    let delta = r.ref_q32 * num / (10_000 * HOUR_MS);
    let (lo, hi) = (r.ref_q32 - delta, (r.ref_q32 + delta).min(SECURE_P_MAX_Q32));
    let pq = u128::from(p) << 32;
    let at = |ref_q32| SecureReferenceVNext {
        ref_q32,
        slot: r.slot.max(slot),
        unix: r.unix.max(unix),
        ..r
    };
    if (lo..=hi).contains(&pq) {
        return at(pq);
    }
    if delta == 0 && mode == SecureStepMode::Fresh {
        return r;
    }
    at(pq.clamp(lo, hi))
}

#[inline(never)]
pub(crate) fn secure_fill(x: u64, p: u64, ref_q32: u128, buy: bool) -> u64 {
    let pq = u128::from(p) << 32;
    let s = U256::from_u128(if buy { pq.max(ref_q32) } else { pq.min(ref_q32) });
    let (pd, one) = (U256::from_u64(p.max(1)), U256::from_u128(SECURE_ONE_Q32));
    let Ok(prod) = s.checked_mul_u64(x) else {
        return if buy { u64::MAX } else { 1 };
    };
    if buy {
        let q = prod.div_ceil_bounded(pd).unwrap_or(U256::MAX);
        let xq = if q.compare(s) == Ordering::Greater { q } else { s };
        xq.div_ceil_bounded(one)
            .and_then(U256::try_u64)
            .unwrap_or(u64::MAX)
    } else {
        let q = prod.div_rem_bounded(pd).map_or(s, |(q, _)| q);
        let xq = if q.compare(s) == Ordering::Less { q } else { s };
        xq.div_rem_bounded(one)
            .map_or(Ok(1), |(q, _)| q.try_u64())
            .unwrap_or(u64::MAX)
            .max(1)
    }
}

#[inline(never)]
pub(crate) fn checked_secure_fill(x: u64, p: u64, ref_q32: u128, buy: bool) -> Result<u64, MathError> {
    let pq = u128::from(p) << 32;
    let s = U256::from_u128(if buy { pq.max(ref_q32) } else { pq.min(ref_q32) });
    let (pd, one) = (U256::from_u64(p.max(1)), U256::from_u128(SECURE_ONE_Q32));
    let prod = s.checked_mul_u64(x)?;
    if buy {
        let q = prod.div_ceil_bounded(pd)?;
        let xq = if q.compare(s) == Ordering::Greater { q } else { s };
        xq.div_ceil_bounded(one)?.try_u64()
    } else {
        let q = prod.div_rem_bounded(pd)?.0;
        let xq = if q.compare(s) == Ordering::Less { q } else { s };
        Ok(xq.div_rem_bounded(one)?.0.try_u64()?.max(1))
    }
}

pub(crate) fn secure_breaker_allows_open(p: u64, ref_q32: u128, buy: bool) -> bool {
    secure_breaker_allows_open_sides(p, ref_q32, buy, SECURE_BREAKER_BOTH_SIDES)
}

fn secure_breaker_allows_open_sides(p: u64, ref_q32: u128, buy: bool, both_sides: bool) -> bool {
    let pq = u128::from(p) << 32;
    pq.abs_diff(ref_q32) * 10_000 <= SECURE_BREAKER_BPS * ref_q32
        || (!both_sides && !(if buy { ref_q32 > pq } else { ref_q32 < pq }))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use pinocchio::error::ProgramError;
    use solana_sha256_hasher::hash;
    use std::{string::String, vec::Vec};

    const FIXTURE: &[u8] = include_bytes!(
        "../../../../../tests/fixtures/permissionless-collateral-s00p/conversion-kernel-r2.json"
    );
    const FIXTURE_SHA256: &str = "ccd8b29d553e17cc35449a811efc201004b25b612868d7d6929def0a3112cd43";

    #[test]
    fn mul_q64_limb_shift_matches_generic_division() {
        fn original(left: u128, right: u128, ceil: bool) -> Result<u128, MathError> {
            if left == 0 || right == 0 {
                return Err(MathError::ZeroPrice);
            }
            let product = U256::from_u128(left).checked_mul_u128(right)?;
            let value = if ceil {
                product.div_ceil_bounded(U256::from_u128(Q64_ONE))?
            } else {
                product.div_rem_bounded(U256::from_u128(Q64_ONE))?.0
            };
            if value.0[2] != 0 || value.0[3] != 0 {
                return Err(MathError::InvalidNarrowing);
            }
            Ok(u128::from(value.0[0]) | (u128::from(value.0[1]) << 64))
        }
        for (left, right) in [
            (0, 1),
            (1, 0),
            (1, 1),
            (Q64_ONE, Q64_ONE),
            (Q64_ONE - 1, Q64_ONE + 1),
            (u128::MAX, Q64_ONE),
            ((1_u128 << 96) - 1, (1_u128 << 96) + 1),
            (1_u128 << 96, 1_u128 << 96),
            (u128::MAX, u128::MAX),
        ] {
            assert_eq!(mul_q64_floor(left, right), original(left, right, false));
            assert_eq!(mul_q64_ceil(left, right), original(left, right, true));
        }
        assert_eq!(mul_q64_floor(1, 1), Ok(0));
        assert_eq!(mul_q64_ceil(1, 1), Ok(1));
        assert_eq!(
            mul_q64_floor((1_u128 << 96) - 1, (1_u128 << 96) + 1),
            Ok(u128::MAX)
        );
        assert_eq!(
            mul_q64_ceil((1_u128 << 96) - 1, (1_u128 << 96) + 1),
            Err(MathError::InvalidNarrowing)
        );
    }

    #[test]
    fn significant_bit_division_boundaries_preserve_floor_remainder_and_ceil() {
        let cases = [
            (
                "zero numerator",
                U256::ZERO,
                U256::from_u64(7),
                Ok((U256::ZERO, U256::ZERO, U256::ZERO)),
            ),
            (
                "zero over zero",
                U256::ZERO,
                U256::ZERO,
                Err(MathError::ZeroDenominator),
            ),
            (
                "nonzero over zero",
                U256::ONE,
                U256::ZERO,
                Err(MathError::ZeroDenominator),
            ),
            (
                "numerator below denominator",
                U256::from_u64(5),
                U256::from_u64(7),
                Ok((U256::ZERO, U256::from_u64(5), U256::ONE)),
            ),
            (
                "exact division",
                U256::from_u64(10),
                U256::from_u64(5),
                Ok((U256::from_u64(2), U256::ZERO, U256::from_u64(2))),
            ),
            (
                "remainder division",
                U256::from_u64(10),
                U256::from_u64(3),
                Ok((U256::from_u64(3), U256::ONE, U256::from_u64(4))),
            ),
            (
                "u128 max over two",
                U256::from_u128(u128::MAX),
                U256::from_u64(2),
                Ok((
                    U256::from_u128(u128::MAX / 2),
                    U256::ONE,
                    U256::from_u128(u128::MAX / 2 + 1),
                )),
            ),
            (
                "u128 max over u128 max",
                U256::from_u128(u128::MAX),
                U256::from_u128(u128::MAX),
                Ok((U256::ONE, U256::ZERO, U256::ONE)),
            ),
            (
                "wide numerator",
                U256::from_limbs_le([0, 0, 1, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_u128(1_u128 << 127),
                    U256::ZERO,
                    U256::from_u128(1_u128 << 127),
                )),
            ),
            (
                "wide denominator",
                U256::from_u128(u128::MAX),
                U256::from_limbs_le([0, 0, 1, 0]),
                Ok((U256::ZERO, U256::from_u128(u128::MAX), U256::ONE)),
            ),
            (
                "wide numerator and denominator",
                U256::from_limbs_le([1, 0, 1, 0]),
                U256::from_limbs_le([0, 0, 1, 0]),
                Ok((U256::ONE, U256::ONE, U256::from_u64(2))),
            ),
            (
                "63 bits",
                U256::from_limbs_le([1_u64 << 62, 0, 0, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([1_u64 << 61, 0, 0, 0]),
                    U256::ZERO,
                    U256::from_limbs_le([1_u64 << 61, 0, 0, 0]),
                )),
            ),
            (
                "64 bits",
                U256::from_limbs_le([1_u64 << 63, 0, 0, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([1_u64 << 62, 0, 0, 0]),
                    U256::ZERO,
                    U256::from_limbs_le([1_u64 << 62, 0, 0, 0]),
                )),
            ),
            (
                "127 bits",
                U256::from_limbs_le([0, 1_u64 << 62, 0, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([0, 1_u64 << 61, 0, 0]),
                    U256::ZERO,
                    U256::from_limbs_le([0, 1_u64 << 61, 0, 0]),
                )),
            ),
            (
                "128 bits",
                U256::from_limbs_le([0, 1_u64 << 63, 0, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([0, 1_u64 << 62, 0, 0]),
                    U256::ZERO,
                    U256::from_limbs_le([0, 1_u64 << 62, 0, 0]),
                )),
            ),
            (
                "191 bits",
                U256::from_limbs_le([0, 0, 1_u64 << 62, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([0, 0, 1_u64 << 61, 0]),
                    U256::ZERO,
                    U256::from_limbs_le([0, 0, 1_u64 << 61, 0]),
                )),
            ),
            (
                "192 bits",
                U256::from_limbs_le([0, 0, 1_u64 << 63, 0]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([0, 0, 1_u64 << 62, 0]),
                    U256::ZERO,
                    U256::from_limbs_le([0, 0, 1_u64 << 62, 0]),
                )),
            ),
            (
                "255 bits",
                U256::from_limbs_le([0, 0, 0, 1_u64 << 62]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([0, 0, 0, 1_u64 << 61]),
                    U256::ZERO,
                    U256::from_limbs_le([0, 0, 0, 1_u64 << 61]),
                )),
            ),
            (
                "top limb",
                U256::from_limbs_le([0, 0, 0, u64::MAX]),
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([0, 0, 1_u64 << 63, u64::MAX >> 1]),
                    U256::ZERO,
                    U256::from_limbs_le([0, 0, 1_u64 << 63, u64::MAX >> 1]),
                )),
            ),
            (
                "full U256",
                U256::MAX,
                U256::from_u64(2),
                Ok((
                    U256::from_limbs_le([u64::MAX, u64::MAX, u64::MAX, u64::MAX >> 1]),
                    U256::ONE,
                    U256::from_limbs_le([0, 0, 0, 1_u64 << 63]),
                )),
            ),
        ];

        for (name, numerator, denominator, expected) in cases {
            match expected {
                Ok((floor, remainder, ceil)) => {
                    assert_eq!(
                        numerator.div_rem_bounded(denominator),
                        Ok((floor, remainder)),
                        "{name}"
                    );
                    assert_eq!(numerator.div_ceil_bounded(denominator), Ok(ceil), "{name}");
                }
                Err(error) => {
                    assert_eq!(numerator.div_rem_bounded(denominator), Err(error), "{name}");
                    assert_eq!(
                        numerator.div_ceil_bounded(denominator),
                        Err(error),
                        "{name}"
                    );
                }
            }
        }
    }

    #[derive(Debug)]
    enum Json<'a> {
        Object(Vec<(&'a str, Json<'a>)>),
        Array(Vec<Json<'a>>),
        String(&'a str),
        Number(&'a str),
        Bool(bool),
        Null,
    }

    impl<'a> Json<'a> {
        fn get(&self, key: &str) -> &Json<'a> {
            match self {
                Self::Object(fields) => {
                    &fields
                        .iter()
                        .find(|(name, _)| *name == key)
                        .unwrap_or_else(|| panic!("missing JSON field {key}"))
                        .1
                }
                _ => panic!("expected JSON object for field {key}"),
            }
        }

        fn optional(&self, key: &str) -> Option<&Json<'a>> {
            match self {
                Self::Object(fields) => fields
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| value),
                _ => panic!("expected JSON object for optional field {key}"),
            }
        }

        fn array(&self) -> &[Json<'a>] {
            match self {
                Self::Array(values) => values,
                _ => panic!("expected JSON array"),
            }
        }

        fn text(&self) -> &'a str {
            match self {
                Self::String(value) | Self::Number(value) => value,
                _ => panic!("expected JSON string or number"),
            }
        }

        fn boolean(&self) -> bool {
            match self {
                Self::Bool(value) => *value,
                _ => panic!("expected JSON boolean"),
            }
        }
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        cursor: usize,
    }

    impl<'a> Parser<'a> {
        fn parse(bytes: &'a [u8]) -> Json<'a> {
            let mut parser = Self { bytes, cursor: 0 };
            let value = parser.value();
            parser.whitespace();
            assert_eq!(parser.cursor, bytes.len(), "trailing fixture JSON");
            value
        }

        fn whitespace(&mut self) {
            while self
                .bytes
                .get(self.cursor)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                self.cursor += 1;
            }
        }

        fn take(&mut self, expected: u8) {
            self.whitespace();
            assert_eq!(self.bytes.get(self.cursor), Some(&expected));
            self.cursor += 1;
        }

        fn literal(&mut self, literal: &[u8]) {
            self.whitespace();
            assert_eq!(
                self.bytes.get(self.cursor..self.cursor + literal.len()),
                Some(literal)
            );
            self.cursor += literal.len();
        }

        fn string(&mut self) -> &'a str {
            self.take(b'"');
            let start = self.cursor;
            while self.bytes[self.cursor] != b'"' {
                assert_ne!(
                    self.bytes[self.cursor], b'\\',
                    "escaped strings are not in the fixture schema"
                );
                self.cursor += 1;
            }
            let value = core::str::from_utf8(&self.bytes[start..self.cursor]).unwrap();
            self.cursor += 1;
            value
        }

        fn number(&mut self) -> &'a str {
            self.whitespace();
            let start = self.cursor;
            while self.bytes.get(self.cursor).is_some_and(|byte| {
                !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}')
            }) {
                self.cursor += 1;
            }
            assert!(self.cursor > start);
            core::str::from_utf8(&self.bytes[start..self.cursor]).unwrap()
        }

        fn value(&mut self) -> Json<'a> {
            self.whitespace();
            match self.bytes[self.cursor] {
                b'{' => {
                    self.cursor += 1;
                    let mut fields = Vec::new();
                    self.whitespace();
                    while self.bytes[self.cursor] != b'}' {
                        let key = self.string();
                        self.take(b':');
                        fields.push((key, self.value()));
                        self.whitespace();
                        if self.bytes[self.cursor] == b',' {
                            self.cursor += 1;
                        } else {
                            break;
                        }
                    }
                    self.take(b'}');
                    Json::Object(fields)
                }
                b'[' => {
                    self.cursor += 1;
                    let mut values = Vec::new();
                    self.whitespace();
                    while self.bytes[self.cursor] != b']' {
                        values.push(self.value());
                        self.whitespace();
                        if self.bytes[self.cursor] == b',' {
                            self.cursor += 1;
                        } else {
                            break;
                        }
                    }
                    self.take(b']');
                    Json::Array(values)
                }
                b'"' => Json::String(self.string()),
                b't' => {
                    self.literal(b"true");
                    Json::Bool(true)
                }
                b'f' => {
                    self.literal(b"false");
                    Json::Bool(false)
                }
                b'n' => {
                    self.literal(b"null");
                    Json::Null
                }
                b'-' | b'0'..=b'9' => Json::Number(self.number()),
                byte => panic!("unexpected JSON byte {byte}"),
            }
        }
    }

    fn fixture() -> Json<'static> {
        let digest = hash(FIXTURE).to_bytes();
        let mut actual = String::with_capacity(64);
        for byte in digest {
            use core::fmt::Write;
            write!(&mut actual, "{byte:02x}").unwrap();
        }
        assert_eq!(actual, FIXTURE_SHA256);
        let root = Parser::parse(FIXTURE);
        assert_eq!(
            root.get("marker").text(),
            "COLLATERAL_S00P_CONVERSION_KERNEL_R2_HOST_ONLY"
        );
        let stage = root.get("stage");
        assert!(!stage.get("runtime_safety").boolean());
        assert!(!stage.get("s01_ready").boolean());
        assert!(!stage.get("abi_reserved").boolean());
        assert!(!stage.get("release_ready").boolean());
        root
    }

    fn rows<'a>(root: &'a Json<'a>, family: &str) -> &'a [Json<'a>] {
        root.get("vectors").get(family).array()
    }

    fn parse_u256(text: &str) -> Option<U256> {
        if text.is_empty()
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let mut value = U256::ZERO;
        for byte in text.bytes() {
            value = value.checked_mul_u64(10).ok()?;
            value = value
                .checked_add(U256::from_u64((byte - b'0') as u64))
                .ok()?;
        }
        Some(value)
    }

    fn u256(row: &Json<'_>, field: &str) -> U256 {
        parse_u256(row.get(field).text())
            .unwrap_or_else(|| panic!("{}.{} is not canonical U256", row.get("id").text(), field))
    }

    fn u64_value(row: &Json<'_>, field: &str) -> u64 {
        u256(row, field)
            .try_u64()
            .unwrap_or_else(|_| panic!("{}.{} is not u64", row.get("id").text(), field))
    }

    fn u128_value(row: &Json<'_>, field: &str) -> u128 {
        let value = u256(row, field);
        assert_eq!(
            value.0[2..],
            [0, 0],
            "{}.{} is not u128",
            row.get("id").text(),
            field
        );
        value.0[0] as u128 | ((value.0[1] as u128) << 64)
    }

    fn decimals(row: &Json<'_>) -> u8 {
        row.get("decimals").text().parse().unwrap()
    }

    fn math_error_code(error: MathError) -> &'static str {
        match error {
            MathError::InvalidDecimals => "INVALID_DECIMALS",
            MathError::ZeroPrice => "ZERO_PRICE",
            MathError::InvertedPriceBounds => "INVERTED_PRICE_BOUNDS",
            MathError::DefaultValueRejected => "DEFAULT_VALUE_REJECTED",
            MathError::IdentityAddressMismatch => "IDENTITY_ADDRESS_MISMATCH",
            MathError::OutOfEnvelope => "OUT_OF_ENVELOPE",
            MathError::U256Overflow => "U256_OVERFLOW",
            MathError::U256Underflow => "U256_UNDERFLOW",
            MathError::ZeroDenominator => "ZERO_DENOMINATOR",
            MathError::InvalidNarrowing => "INVALID_NARROWING",
            MathError::InsufficientFreeAtoms => "INSUFFICIENT_FREE_ATOMS",
        }
    }

    fn assert_error<T>(result: Result<T, MathError>, row: &Json<'_>) {
        let error = result
            .err()
            .unwrap_or_else(|| panic!("{} unexpectedly succeeded", row.get("id").text()));
        assert_eq!(
            math_error_code(error),
            row.get("error").text(),
            "{}",
            row.get("id").text()
        );
    }

    fn prices(row: &Json<'_>) -> Result<PriceBoundsQ64, MathError> {
        checked_price_bounds(u128_value(row, "p_lo_q64"), u128_value(row, "p_hi_q64"))
    }

    fn address(row: &Json<'_>, field: &str) -> [u8; 32] {
        let text = row.get(field).text().as_bytes();
        assert_eq!(text.len(), 64);
        let mut out = [0_u8; 32];
        for (index, slot) in out.iter_mut().enumerate() {
            let nibble = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("non-lowercase-hex fixture address"),
            };
            *slot = (nibble(text[index * 2]) << 4) | nibble(text[index * 2 + 1]);
        }
        out
    }

    #[test]
    fn frozen_fixture_and_all_92_economic_and_rejection_rows_call_production() {
        let root = fixture();
        let counts = [
            ("quote_floor", 18),
            ("atoms_floor_ceil", 16),
            ("credited_quote", 8),
            ("impairment_bound", 11),
            ("authenticated_cap", 5),
            ("open_requirement", 9),
            ("final_debit_partition", 9),
            ("identity_basis", 4),
            ("rejections", 12),
        ];
        assert_eq!(counts.iter().map(|(_, count)| count).sum::<usize>(), 92);
        for (family, expected) in counts {
            assert_eq!(rows(&root, family).len(), expected, "{family}");
        }

        for row in rows(&root, "quote_floor") {
            assert_eq!(
                quote_e6_floor_wide(
                    u64_value(row, "atoms"),
                    u128_value(row, "price_q64"),
                    decimals(row)
                )
                .unwrap(),
                u256(row, "quote_e6_floor_wide"),
                "{}",
                row.get("id").text()
            );
        }
        for row in rows(&root, "atoms_floor_ceil") {
            let quote = u256(row, "quote_e6_wide");
            let price = u128_value(row, "price_q64");
            let decimals = decimals(row);
            let (numerator, denominator) = atom_ratio_wide(quote, price, decimals).unwrap();
            let (floor, remainder) = numerator.div_rem_bounded(denominator).unwrap();
            assert_eq!(
                atoms_payout_floor_wide(quote, price, decimals).unwrap(),
                floor
            );
            assert_eq!(
                floor,
                u256(row, "atoms_payout_floor_wide"),
                "{}",
                row.get("id").text()
            );
            assert_eq!(
                atoms_debit_ceil_wide(quote, price, decimals).unwrap(),
                u256(row, "atoms_debit_ceil_wide"),
                "{}",
                row.get("id").text()
            );
            assert_eq!(
                remainder,
                u256(row, "division_remainder"),
                "{}",
                row.get("id").text()
            );
        }
        for row in rows(&root, "credited_quote") {
            assert_eq!(
                credited_quote_e6_wide(
                    u64_value(row, "atoms"),
                    u128_value(row, "p_lo_q64"),
                    decimals(row)
                )
                .unwrap(),
                u256(row, "credited_quote_e6_wide"),
                "{}",
                row.get("id").text()
            );
        }
        for row in rows(&root, "impairment_bound") {
            let price = u128_value(row, "p_hi_q64");
            assert_eq!(
                displayed_subatom_impairment_bound_quote_e6(
                    checked_price_bounds(price, price).unwrap(),
                    decimals(row)
                )
                .unwrap(),
                u256(row, "bound_quote_e6"),
                "{}",
                row.get("id").text()
            );
        }
        for row in rows(&root, "authenticated_cap") {
            assert_eq!(
                cap_quote_wide(
                    u256(row, "value_quote_e6_wide"),
                    AuthenticatedQuoteCap::from_authenticated_parent(u64_value(
                        row,
                        "authenticated_cap_quote_e6"
                    ))
                ),
                u64_value(row, "capped_quote_e6"),
                "{}",
                row.get("id").text()
            );
        }
        for row in rows(&root, "open_requirement") {
            let result = prices(row).and_then(|bounds| {
                open_pre_haircut_requirement(
                    u64_value(row, "initial_margin_quote_e6"),
                    bounds,
                    decimals(row),
                    u64_value(row, "free_atoms"),
                )
            });
            if row.optional("error").is_some() {
                assert_error(result, row);
            } else {
                let actual = result.unwrap();
                assert_eq!(
                    actual.required_pre_haircut_quote_e6_wide,
                    u256(row, "required_pre_haircut_quote_e6_wide"),
                    "{}",
                    row.get("id").text()
                );
                assert_eq!(
                    actual.required_allocation_atoms,
                    u64_value(row, "required_allocation_atoms"),
                    "{}",
                    row.get("id").text()
                );
            }
        }
        for row in rows(&root, "final_debit_partition") {
            let result = prices(row).and_then(|bounds| {
                final_debit_partition(
                    u64_value(row, "loss_quote_e6"),
                    bounds,
                    decimals(row),
                    u64_value(row, "eligible_atoms"),
                )
            });
            if row.optional("error").is_some() {
                assert_error(result, row);
            } else {
                let actual = result.unwrap();
                assert_eq!(
                    actual.theoretical_debit_atoms_wide,
                    u256(row, "theoretical_debit_atoms_wide"),
                    "{}",
                    row.get("id").text()
                );
                assert_eq!(
                    actual.debited_atoms,
                    u64_value(row, "debited_atoms"),
                    "{}",
                    row.get("id").text()
                );
                assert_eq!(
                    actual.covered_loss_quote_e6,
                    u64_value(row, "covered_loss_quote_e6"),
                    "{}",
                    row.get("id").text()
                );
                assert_eq!(
                    actual.uncovered_loss_quote_e6,
                    u64_value(row, "uncovered_loss_quote_e6"),
                    "{}",
                    row.get("id").text()
                );
            }
        }
        for row in rows(&root, "identity_basis") {
            let collateral = address(row, "collateral_mint_hex");
            let quote = address(row, "quote_mint_hex");
            let result = identity_price_bounds_by_address_equality(
                &collateral,
                &quote,
                row.get("authenticated_mint_decimals")
                    .text()
                    .parse()
                    .unwrap(),
            );
            if row.optional("error").is_some() {
                assert_error(result, row);
            } else {
                let actual = result.unwrap();
                assert_eq!(actual.p_lo, Q64_ONE);
                assert_eq!(actual.p_hi, Q64_ONE);
            }
        }
        for row in rows(&root, "rejections") {
            let result = match row.get("operation").text() {
                "quote_e6_floor_wide" => quote_e6_floor_wide(
                    u64_value(row, "atoms"),
                    u128_value(row, "price_q64"),
                    decimals(row),
                )
                .map(|_| ()),
                "atoms_pair_wide" => atom_ratio_wide(
                    u256(row, "quote_e6_wide"),
                    u128_value(row, "price_q64"),
                    decimals(row),
                )
                .map(|_| ()),
                "displayed_subatom_impairment_bound_quote_e6" => {
                    let price = u128_value(row, "p_hi_q64");
                    checked_price_bounds(price, price)
                        .and_then(|bounds| {
                            displayed_subatom_impairment_bound_quote_e6(bounds, decimals(row))
                        })
                        .map(|_| ())
                }
                "checked_price_bounds" => {
                    checked_price_bounds(u128_value(row, "p_lo_q64"), u128_value(row, "p_hi_q64"))
                        .map(|_| ())
                }
                operation => panic!("unknown rejection operation {operation}"),
            };
            assert_error(result, row);
        }
    }

    #[test]
    fn all_13_primitive_rows_call_production_primitives() {
        let root = fixture();
        let rows = rows(&root, "u256_primitive_edges");
        assert_eq!(rows.len(), 13);
        for row in rows {
            let result: Result<(), MathError> = match row.get("operation").text() {
                "checked_add" => {
                    let result = u256(row, "left").checked_add(u256(row, "right"));
                    if row.optional("sum").is_some() {
                        result.map(|value| assert_eq!(value, u256(row, "sum")))
                    } else {
                        result.map(|_| ())
                    }
                }
                "checked_sub" => {
                    let result = u256(row, "left").checked_sub(u256(row, "right"));
                    if row.optional("difference").is_some() {
                        result.map(|value| assert_eq!(value, u256(row, "difference")))
                    } else {
                        result.map(|_| ())
                    }
                }
                "checked_mul_u64" => u256(row, "left")
                    .checked_mul_u64(u64_value(row, "right"))
                    .map(|_| ()),
                "checked_mul_u128" => u256(row, "left")
                    .checked_mul_u128(u128_value(row, "right"))
                    .map(|_| ()),
                "checked_shl_64" => {
                    let result = u256(row, "value").checked_shl_64();
                    if row.optional("shifted").is_some() {
                        result.map(|value| assert_eq!(value, u256(row, "shifted")))
                    } else {
                        result.map(|_| ())
                    }
                }
                "try_u64" => {
                    let result = u256(row, "value").try_u64();
                    if row.optional("u64").is_some() {
                        result.map(|value| assert_eq!(value, u64_value(row, "u64")))
                    } else {
                        result.map(|_| ())
                    }
                }
                "div_rem_bounded" => {
                    let result = u256(row, "numerator").div_rem_bounded(u256(row, "denominator"));
                    if row.optional("quotient").is_some() {
                        result.map(|(quotient, remainder)| {
                            assert_eq!(quotient, u256(row, "quotient"));
                            assert_eq!(remainder, u256(row, "remainder"));
                        })
                    } else {
                        result.map(|_| ())
                    }
                }
                "compare" => {
                    assert_eq!(
                        u256(row, "left").compare(u256(row, "right")),
                        Ordering::Greater
                    );
                    assert_eq!(row.get("comparison").text(), "greater");
                    Ok(())
                }
                operation => panic!("unknown primitive operation {operation}"),
            };
            if row.optional("error").is_some() {
                assert_error(result, row);
            } else {
                result.unwrap();
            }
        }
    }

    #[test]
    fn all_six_validation_precedence_rows_are_bound() {
        let root = fixture();
        let rows = rows(&root, "validation_precedence");
        assert_eq!(rows.len(), 6);
        for row in rows {
            let result = match row.get("id").text() {
                "precedence_range_before_decimals" => {
                    assert_eq!(row.get("error").text(), "INPUT_OUT_OF_RANGE");
                    assert!(parse_u256(row.get("atoms").text()).is_none());
                    continue;
                }
                "precedence_decimals_before_price" => quote_e6_floor_wide(1, 0, 5).map(|_| ()),
                "precedence_price_before_envelope" => {
                    atom_ratio_wide(u256(row, "quote_e6_wide"), 0, 6).map(|_| ())
                }
                "precedence_envelope_before_multiplication" => {
                    atom_ratio_wide(u256(row, "quote_e6_wide"), 1, 6).map(|_| ())
                }
                "precedence_narrowing_before_availability" => open_pre_haircut_requirement(
                    u64_value(row, "initial_margin_quote_e6"),
                    prices(row).unwrap(),
                    6,
                    0,
                )
                .map(|_| ()),
                "precedence_public_zero_price_before_private_denominator" => {
                    atom_ratio_wide(U256::ONE, 0, 6).map(|_| ())
                }
                id => panic!("unknown precedence row {id}"),
            };
            assert_error(result, row);
        }
    }

    #[test]
    fn both_exact_168_case_grids_and_all_four_qmax_boundaries_hold() {
        let decimals_values = [6_u8, 7, 8, 9];
        let atoms_values = [0_u64, 1, 2, 999_999, 1_000_000, u64::MAX - 1, u64::MAX];
        let quote_values = [0_u64, 1, 2, 999_999, 1_000_000, u64::MAX - 1, u64::MAX];
        let prices = [
            1_u128,
            Q64_ONE - 1,
            Q64_ONE,
            Q64_ONE + 1,
            u128::MAX - 1,
            u128::MAX,
        ];
        let mut quote_cases = 0;
        let mut atom_cases = 0;
        for decimals in decimals_values {
            for price in prices {
                let mut previous_quote = U256::ZERO;
                for atoms in atoms_values {
                    let quote = quote_e6_floor_wide(atoms, price, decimals).unwrap();
                    assert!(quote.compare(previous_quote) != Ordering::Less);
                    previous_quote = quote;
                    quote_cases += 1;
                }
                let mut previous_floor = U256::ZERO;
                let mut previous_ceil = U256::ZERO;
                for quote in quote_values {
                    let quote = U256::from_u64(quote);
                    let floor = atoms_payout_floor_wide(quote, price, decimals).unwrap();
                    let ceil = atoms_debit_ceil_wide(quote, price, decimals).unwrap();
                    assert!(floor.compare(previous_floor) != Ordering::Less);
                    assert!(ceil.compare(previous_ceil) != Ordering::Less);
                    assert!(ceil == floor || ceil == floor.checked_add(U256::ONE).unwrap());
                    let (numerator, denominator) = atom_ratio_wide(quote, price, decimals).unwrap();
                    let (division_floor, remainder) =
                        numerator.div_rem_bounded(denominator).unwrap();
                    assert_eq!(floor, division_floor);
                    assert!(remainder.compare(denominator) == Ordering::Less);
                    assert_eq!(
                        numerator,
                        floor
                            .checked_mul_u128(price)
                            .unwrap()
                            .checked_mul_u64(QUOTE_E6_SCALE)
                            .unwrap()
                            .checked_add(remainder)
                            .unwrap()
                    );
                    if floor.compare(U256::from_u64(u64::MAX)) != Ordering::Greater {
                        assert!(
                            quote_e6_floor_wide(floor.try_u64().unwrap(), price, decimals)
                                .unwrap()
                                .compare(quote)
                                != Ordering::Greater
                        );
                    }
                    if ceil.compare(U256::from_u64(u64::MAX)) != Ordering::Greater {
                        assert!(
                            quote_e6_floor_wide(ceil.try_u64().unwrap(), price, decimals)
                                .unwrap()
                                .compare(quote)
                                != Ordering::Less
                        );
                    }
                    previous_floor = floor;
                    previous_ceil = ceil;
                    atom_cases += 1;
                }
            }
        }
        assert_eq!(quote_cases, 168);
        assert_eq!(atom_cases, 168);

        for decimals in decimals_values {
            for atoms in atoms_values {
                let mut previous = U256::ZERO;
                for price in prices {
                    let value = quote_e6_floor_wide(atoms, price, decimals).unwrap();
                    assert!(value.compare(previous) != Ordering::Less);
                    previous = value;
                }
            }
            for quote in quote_values {
                let mut previous_floor = None;
                let mut previous_ceil = None;
                for price in prices {
                    let quote = U256::from_u64(quote);
                    let floor = atoms_payout_floor_wide(quote, price, decimals).unwrap();
                    let ceil = atoms_debit_ceil_wide(quote, price, decimals).unwrap();
                    if let Some(previous) = previous_floor {
                        assert!(floor.compare(previous) != Ordering::Greater);
                    }
                    if let Some(previous) = previous_ceil {
                        assert!(ceil.compare(previous) != Ordering::Greater);
                    }
                    previous_floor = Some(floor);
                    previous_ceil = Some(ceil);
                }
            }
            let factor = U256::from_u64(collateral_scale(decimals).unwrap())
                .checked_shl_64()
                .unwrap();
            let q_max = U256::MAX.div_rem_bounded(factor).unwrap().0;
            atoms_payout_floor_wide(q_max, 1, decimals).unwrap();
            assert_eq!(
                atoms_payout_floor_wide(q_max.checked_add(U256::ONE).unwrap(), 1, decimals),
                Err(MathError::OutOfEnvelope)
            );
        }

        for atoms in atoms_values {
            for price in prices {
                let mut previous = None;
                for decimals in decimals_values {
                    let value = quote_e6_floor_wide(atoms, price, decimals).unwrap();
                    if let Some(previous) = previous {
                        assert!(value.compare(previous) != Ordering::Greater);
                    }
                    previous = Some(value);
                }
            }
        }
        for quote in quote_values {
            for price in prices {
                let mut previous_floor = None;
                let mut previous_ceil = None;
                for decimals in decimals_values {
                    let quote = U256::from_u64(quote);
                    let floor = atoms_payout_floor_wide(quote, price, decimals).unwrap();
                    let ceil = atoms_debit_ceil_wide(quote, price, decimals).unwrap();
                    if let Some(previous) = previous_floor {
                        assert!(floor.compare(previous) != Ordering::Less);
                    }
                    if let Some(previous) = previous_ceil {
                        assert!(ceil.compare(previous) != Ordering::Less);
                    }
                    previous_floor = Some(floor);
                    previous_ceil = Some(ceil);
                }
            }
        }
    }

    #[test]
    fn identity_impairment_open_reject_final_clamp_and_error_numbers_are_explicit() {
        let mint = [1_u8; 32];
        assert_eq!(
            identity_price_bounds_by_address_equality(&mint, &mint, 6).unwrap(),
            PriceBoundsQ64 {
                p_lo: 1_u128 << 64,
                p_hi: 1_u128 << 64
            }
        );
        let identity = checked_price_bounds(Q64_ONE, Q64_ONE).unwrap();
        assert_eq!(
            displayed_subatom_impairment_bound_quote_e6(identity, 6).unwrap(),
            U256::ONE
        );
        assert_eq!(
            open_pre_haircut_requirement(1, identity, 6, 1),
            Err(MathError::InsufficientFreeAtoms)
        );
        let clamped =
            final_debit_partition(1, checked_price_bounds(1, 1).unwrap(), 9, u64::MAX).unwrap();
        assert!(
            clamped
                .theoretical_debit_atoms_wide
                .compare(U256::from_u64(u64::MAX))
                == Ordering::Greater
        );
        assert_eq!(clamped.debited_atoms, u64::MAX);
        assert_eq!(
            clamped.covered_loss_quote_e6 + clamped.uncovered_loss_quote_e6,
            1
        );
        assert_eq!(U256::from_limbs_le([1, 2, 3, 4]), U256([1, 2, 3, 4]));

        let mappings = [
            (MathError::InvalidDecimals, 72),
            (MathError::DefaultValueRejected, 72),
            (MathError::IdentityAddressMismatch, 73),
            (MathError::ZeroPrice, 29),
            (MathError::InvertedPriceBounds, 29),
            (MathError::OutOfEnvelope, 12),
            (MathError::U256Overflow, 12),
            (MathError::ZeroDenominator, 12),
            (MathError::InvalidNarrowing, 12),
            (MathError::U256Underflow, 13),
            (MathError::InsufficientFreeAtoms, 22),
        ];
        for (error, code) in mappings {
            assert_eq!(ProgramError::from(error), ProgramError::Custom(code));
        }
    }

    #[test]
    fn d2s_kernel_matches_secure_reference_vectors() {
        let steps: [(u128, u128, u64, i64, u64, u64, i64, SecureStepMode, u128, u64, i64); 57] = [
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000000, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000002, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000003, 1790000001, SecureStepMode::Fresh, 4294991156929422222, 400000003, 1790000001),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000025, 1790000010, SecureStepMode::Fresh, 4295205905294222222, 400000025, 1790000010),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400004500, 1790001800, SecureStepMode::Fresh, 4337916968960000000, 400004500, 1790001800),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400648000, 1790259200, SecureStepMode::Fresh, 4337916968960000000, 400648000, 1790259200),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000010, 1790003600, SecureStepMode::Fresh, 4295062739717688888, 400000010, 1790003600),
            (200, 4294967296000000000, 400000000, 1790000000, 500000000, 400004500, 1790001800, SecureStepMode::Fresh, 4252017623040000000, 400004500, 1790001800),
            (200, 4294967296000000000, 400000000, 1790000000, 1001000000, 400004500, 1790001800, SecureStepMode::Fresh, 4299262263296000000, 400004500, 1790001800),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400004500, 1789999995, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000002, 1790000000, SecureStepMode::Open, 4294967296000000000, 400000002, 1790000000),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400004500, 1790001800, SecureStepMode::Frozen, 4294967296000000000, 400000000, 1790000000),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400004500, 1790001800, SecureStepMode::Frozen, 4294967296000000000, 400000000, 1790000000),
            (200, 425201762304, 400000000, 1790000000, 200, 400648000, 1790259200, SecureStepMode::Fresh, 429453779927, 400648000, 1790259200),
            (200, 4294967296, 400000000, 1790000000, 2, 400000001, 1790000001, SecureStepMode::Fresh, 4294976840, 400000001, 1790000001),
            (200, 79228162514264337589248983040, 400000000, 1790000000, 18446744073709551614, 400004500, 1790001800, SecureStepMode::Fresh, 79228162514264337584954015744, 400004500, 1790001800),
            (200, 79228158219297041589248983040, 400000000, 1790000000, 18446744073709551615, 400004500, 1790001800, SecureStepMode::Fresh, 79228162514264337589248983040, 400004500, 1790001800),
            (200, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000001, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (200, 4294967296000, 400000000, 1790000000, 2000, 400004500, 1790001800, SecureStepMode::Fresh, 4337916968960, 400004500, 1790001800),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000000, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000002, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000003, 1790000001, SecureStepMode::Fresh, 4295026948323555555, 400000003, 1790000001),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000025, 1790000010, SecureStepMode::Fresh, 4295563819235555555, 400000025, 1790000010),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400001800, 1790000720, SecureStepMode::Fresh, 4337916968960000000, 400001800, 1790000720),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400648000, 1790259200, SecureStepMode::Fresh, 4337916968960000000, 400648000, 1790259200),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000010, 1790003600, SecureStepMode::Fresh, 4295205905294222222, 400000010, 1790003600),
            (500, 4294967296000000000, 400000000, 1790000000, 500000000, 400001800, 1790000720, SecureStepMode::Fresh, 4252017623040000000, 400001800, 1790000720),
            (500, 4294967296000000000, 400000000, 1790000000, 1001000000, 400001800, 1790000720, SecureStepMode::Fresh, 4299262263296000000, 400001800, 1790000720),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400001800, 1789999995, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000002, 1790000000, SecureStepMode::Open, 4294967296000000000, 400000002, 1790000000),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400001800, 1790000720, SecureStepMode::Frozen, 4294967296000000000, 400000000, 1790000000),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400001800, 1790000720, SecureStepMode::Frozen, 4294967296000000000, 400000000, 1790000000),
            (500, 425201762304, 400000000, 1790000000, 200, 400648000, 1790259200, SecureStepMode::Fresh, 429453779927, 400648000, 1790259200),
            (500, 4294967296, 400000000, 1790000000, 2, 400000001, 1790000001, SecureStepMode::Fresh, 4294991156, 400000001, 1790000001),
            (500, 79228162514264337589248983040, 400000000, 1790000000, 18446744073709551614, 400001800, 1790000720, SecureStepMode::Fresh, 79228162514264337584954015744, 400001800, 1790000720),
            (500, 79228158219297041589248983040, 400000000, 1790000000, 18446744073709551615, 400001800, 1790000720, SecureStepMode::Fresh, 79228162514264337589248983040, 400001800, 1790000720),
            (500, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000001, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (500, 4294967296000, 400000000, 1790000000, 2000, 400001800, 1790000720, SecureStepMode::Fresh, 4337916968960, 400001800, 1790000720),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000000, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000002, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000003, 1790000001, SecureStepMode::Fresh, 4295086600647111111, 400000003, 1790000001),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000025, 1790000010, SecureStepMode::Fresh, 4296160342471111111, 400000025, 1790000010),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000900, 1790000360, SecureStepMode::Fresh, 4337916968960000000, 400000900, 1790000360),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400648000, 1790259200, SecureStepMode::Fresh, 4337916968960000000, 400648000, 1790259200),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000010, 1790003600, SecureStepMode::Fresh, 4295444514588444444, 400000010, 1790003600),
            (1000, 4294967296000000000, 400000000, 1790000000, 500000000, 400000900, 1790000360, SecureStepMode::Fresh, 4252017623040000000, 400000900, 1790000360),
            (1000, 4294967296000000000, 400000000, 1790000000, 1001000000, 400000900, 1790000360, SecureStepMode::Fresh, 4299262263296000000, 400000900, 1790000360),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000900, 1789999995, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000002, 1790000000, SecureStepMode::Open, 4294967296000000000, 400000002, 1790000000),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000900, 1790000360, SecureStepMode::Frozen, 4294967296000000000, 400000000, 1790000000),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000900, 1790000360, SecureStepMode::Frozen, 4294967296000000000, 400000000, 1790000000),
            (1000, 425201762304, 400000000, 1790000000, 200, 400648000, 1790259200, SecureStepMode::Fresh, 429453779927, 400648000, 1790259200),
            (1000, 4294967296, 400000000, 1790000000, 2, 400000001, 1790000001, SecureStepMode::Fresh, 4295015017, 400000001, 1790000001),
            (1000, 79228162514264337589248983040, 400000000, 1790000000, 18446744073709551614, 400000900, 1790000360, SecureStepMode::Fresh, 79228162514264337584954015744, 400000900, 1790000360),
            (1000, 79228158219297041589248983040, 400000000, 1790000000, 18446744073709551615, 400000900, 1790000360, SecureStepMode::Fresh, 79228162514264337589248983040, 400000900, 1790000360),
            (1000, 4294967296000000000, 400000000, 1790000000, 2000000000, 400000001, 1790000000, SecureStepMode::Fresh, 4294967296000000000, 400000000, 1790000000),
            (1000, 4294967296000, 400000000, 1790000000, 2000, 400000900, 1790000360, SecureStepMode::Fresh, 4337916968960, 400000900, 1790000360),
        ];
        for (rate, ref_q32, slot0, unix0, p, slot, unix, mode, q, s, u) in steps {
            let r = SecureReferenceVNext { lifted: true, ref_q32, slot: slot0, unix: unix0 };
            assert_eq!(
                secure_reference_step_at(rate, r, p, slot, unix, mode),
                SecureReferenceVNext { lifted: true, ref_q32: q, slot: s, unix: u },
                "{rate} {ref_q32} {p} {slot} {unix}"
            );
        }
        let fills: [(u64, u64, u128, bool, u64); 17] = [
            (1003000000, 1000000000, 4509715660800000000, true, 1053150000),
            (1003000000, 1000000000, 4080218931200000000, true, 1003000000),
            (626708498, 1000000000, 4294967296000000000, true, 1000000000),
            (997000000, 1000000000, 4080218931200000000, false, 947150000),
            (1367290592, 1000000000, 4294967296000000000, false, 1000000000),
            (1995000000, 2000000000, 4337916968960000000, false, 1007475000),
            (501000000, 500000000, 4252017623040000000, true, 991980000),
            (1200000000, 1200000000, 4724464025600000000, false, 1100000000),
            (1200000000, 1200000000, 5583457484800000000, true, 1300000000),
            (1000, 1000, 4294967296001, true, 1001),
            (1000, 1000, 4294967295999, false, 999),
            (1, 2, 4294967296, false, 1),
            (1, 1, 4294967296, true, 1),
            (18446744073709551615, 4611686018427387904, 39614081257132168796771975168, true, 18446744073709551615),
            (18446744073709551615, 18446744073709551615, 79228162514264337589248983040, false, 18446744073709551615),
            (7, 3, 17179869184, true, 10),
            (7, 4, 12884901888, false, 3),
        ];
        for (x, p, ref_q32, buy, expect) in fills {
            assert_eq!(secure_fill(x, p, ref_q32, buy), expect, "{x} {p} {ref_q32} {buy}");
            assert!(checked_secure_fill(x, p, ref_q32, buy).map_or(expect == u64::MAX, |fill| fill == expect));
        }
        assert_eq!(checked_secure_fill(101, 100, SECURE_P_MAX_Q32, true), Err(MathError::InvalidNarrowing));
        assert_eq!(checked_secure_fill(u64::MAX, 4611686018427387904, 39614081257132168796771975168, true), Err(MathError::InvalidNarrowing));
        assert_eq!(checked_secure_fill(u64::MAX, u64::MAX, SECURE_P_MAX_Q32, false), Ok(u64::MAX));
        let breakers: [(u64, u128, bool, bool, bool); 9] = [
            (1050000000, 4294967296000000000, true, true, true),
            (1050000001, 4294967296000000000, true, false, true),
            (1050000001, 4294967296000000000, false, false, false),
            (949999999, 4294967296000000000, true, false, false),
            (949999999, 4294967296000000000, false, false, true),
            (2000000000, 4337916968960000000, true, false, true),
            (2000000000, 4337916968960000000, false, false, false),
            (200, 429453779927, true, false, true),
            (200, 429453779927, false, false, false),
        ];
        for (p, ref_q32, buy, both, one) in breakers {
            assert_eq!(secure_breaker_allows_open_sides(p, ref_q32, buy, true), both);
            assert_eq!(secure_breaker_allows_open_sides(p, ref_q32, buy, false), one);
            assert_eq!(secure_breaker_allows_open(p, ref_q32, buy), both);
        }
        let caps: [(u128, (u128, u128), (u128, u128)); 9] = [
            (0, (0, 0), (0, 0)),
            (145046, (3626150, 5076610), (14504, 36261)),
            (1448615, (36215375, 50701525), (144861, 362153)),
            (10000000, (250000000, 350000000), (1000000, 2500000)),
            (20000000, (250000000, 700000000), (2000000, 5000000)),
            (28571428, (250000000, 999999980), (2857142, 7142857)),
            (28571429, (250000000, 1000000000), (2857142, 7142857)),
            (100000000, (250000000, 1000000000), (10000000, 25000000)),
            (1000000000, (250000000, 1000000000), (100000000, 250000000)),
        ];
        for (floor, secure, interim) in caps {
            use crate::instructions::collateral_vnext_oracle::mixed_bounded::point_caps;
            assert_eq!(point_caps(floor, true), secure);
            assert_eq!(point_caps(floor, false), interim);
        }
        let n = 250_000_000;
        let open = secure_reference_step(
            SecureReferenceVNext { lifted: true, ref_q32: 1_000_000_000 << 32, slot: 100 - 1, unix: 10 },
            2_000_000_000,
            100,
            10,
            SecureStepMode::Open,
        );
        let close = secure_reference_step(open, 2_000_000_000, 103, 11, SecureStepMode::Fresh);
        let entry = secure_fill(1_000_000_000, 1_000_000_000, open.ref_q32, true);
        let exit = secure_fill(2_000_000_000, 2_000_000_000, close.ref_q32, false);
        let (_, gain) = funded_net_and_capped(n, entry, exit, 0, 0, n as u64, false).unwrap();
        assert_eq!(gain, 1388);
        let (_, debit_only) = funded_net_and_capped(-n, 1_000_000_000, 1_000_000_000, 0, 1 << 70, n as u64, false).unwrap();
        assert_eq!(debit_only, 0);
        let (_, debit) = funded_net_and_capped(n, 1_000_000_000, 1_000_000_000, 0, 1 << 70, n as u64, false).unwrap();
        assert!(debit < 0);
    }

    #[test]
    fn d2_funded_net_matches_n613_reference_vectors() {
        const CASES: [(i64, u64, u64, i128, i128, u64, i128, i128); 14] = [
            (250000000, 1000, 35925, 0, 0, 100000000, 8731250000, 250000000),
            (-250000000, 1000, 35925, 0, 0, 100000000, -8731250000, -100000000),
            (-250000000, 1000000000, 1, 0, 0, 100000000, 249999999, 249999999),
            (250000000, 1000, 1000, 0, -36893488147419103232, 100000000, 500000000, 250000000),
            (250000000, 1, 4294967296, 0, 79228162495817593519834398720, 100000000, 0, 0),
            (1, 3, 2, 0, 0, 1, -1, -1),
            (1, 3, 4, 0, 0, 1, 0, 0),
            (1, 1, 1, 0, 1, 1, -1, -1),
            (1, 1, 1, 0, -1, 1, 0, 0),
            (17592186044416, 1, u64::MAX, 0, 0, u64::MAX, 324518553658426726747971648487424, 17592186044416),
            (-17592186044416, 1, 1, i128::MIN, i128::MAX, u64::MAX, 324518553658426726783156020576255, 17592186044416),
            (17592186044416, 1, 1, i128::MIN, i128::MAX, u64::MAX, -324518553658426726783156020576256, -18446744073709551615),
            (17592186044416, 1, u64::MAX, 0, i128::MAX, u64::MAX, 162259276829213363356393638199296, 17592186044416),
            (-250000000, 1, u64::MAX, 0, 0, 0, -4611686018427387903500000000, 0),
        ];
        for (size, entry, exit, checkpoint, current, allocated, raw, capped) in CASES {
            assert_eq!(
                funded_net_and_capped(size, entry, exit, checkpoint, current, allocated, true),
                Ok((raw, capped)),
                "{size} {entry} {exit} {checkpoint} {current} {allocated}"
            );
        }
        assert!(funded_net_and_capped(0, 1, 1, 0, 0, 1, true).is_err());
        assert!(funded_net_and_capped(1 << 44 | 1, 1, 1, 0, 0, 1, true).is_err());
        assert!(funded_net_and_capped(1, 0, 1, 0, 0, 1, true).is_err());
        assert!(funded_net_and_capped(1, 1, 0, 0, 0, 1, true).is_err());
    }
}
