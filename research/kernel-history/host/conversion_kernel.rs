use core::cmp::Ordering;
use solana_sha256_hasher::hash;

// COLLATERAL_S00P_CONVERSION_KERNEL_R2_HOST_ONLY
// runtime_safety=false; s01_ready=false; abi_reserved=false; release_ready=false

type CollateralAtoms = u64;
type QuoteE6 = u64;
type SignedQuoteE6 = i128;
type PriceQ64 = u128;
type QuoteE6Wide = HostU256;

const Q64_ONE: u128 = 1_u128 << 64;
const QUOTE_E6_SCALE: u64 = 1_000_000;
const BPS_DENOMINATOR: u64 = 10_000;
const COLLATERAL_FACTOR_BPS: u64 = 5_000;
const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/permissionless-collateral-s00p/conversion-kernel-r2.json");
const FIXTURE_SHA256: &str = "ccd8b29d553e17cc35449a811efc201004b25b612868d7d6929def0a3112cd43";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelError {
    InputOutOfRange,
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
impl KernelError {
    fn code(self) -> &'static str {
        match self {
            Self::InputOutOfRange => "INPUT_OUT_OF_RANGE",
            Self::InvalidDecimals => "INVALID_DECIMALS",
            Self::ZeroPrice => "ZERO_PRICE",
            Self::InvertedPriceBounds => "INVERTED_PRICE_BOUNDS",
            Self::DefaultValueRejected => "DEFAULT_VALUE_REJECTED",
            Self::IdentityAddressMismatch => "IDENTITY_ADDRESS_MISMATCH",
            Self::OutOfEnvelope => "OUT_OF_ENVELOPE",
            Self::U256Overflow => "U256_OVERFLOW",
            Self::U256Underflow => "U256_UNDERFLOW",
            Self::ZeroDenominator => "ZERO_DENOMINATOR",
            Self::InvalidNarrowing => "INVALID_NARROWING",
            Self::InsufficientFreeAtoms => "INSUFFICIENT_FREE_ATOMS",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostU256([u64; 4]);
impl HostU256 {
    const ZERO: Self = Self([0; 4]);
    const ONE: Self = Self([1, 0, 0, 0]);
    const MAX: Self = Self([u64::MAX; 4]);

    const fn from_u64(value: u64) -> Self { Self([value, 0, 0, 0]) }
    const fn from_u128(value: u128) -> Self { Self([value as u64, (value >> 64) as u64, 0, 0]) }
    const fn from_limbs_le(value: [u64; 4]) -> Self { Self(value) }

    fn compare(self, rhs: Self) -> Ordering {
        for index in (0..4).rev() {
            match self.0[index].cmp(&rhs.0[index]) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        Ordering::Equal
    }
    fn checked_add(self, rhs: Self) -> Result<Self, KernelError> {
        let mut out = [0_u64; 4];
        let mut carry = 0_u128;
        for (index, slot) in out.iter_mut().enumerate() {
            let sum = self.0[index] as u128 + rhs.0[index] as u128 + carry;
            *slot = sum as u64;
            carry = sum >> 64;
        }
        if carry != 0 { return Err(KernelError::U256Overflow); }
        Ok(Self(out))
    }
    fn checked_sub(self, rhs: Self) -> Result<Self, KernelError> {
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
        if borrow != 0 { return Err(KernelError::U256Underflow); }
        Ok(Self(out))
    }
    fn checked_mul_u64(self, rhs: u64) -> Result<Self, KernelError> {
        let mut out = [0_u64; 4];
        let mut carry = 0_u128;
        for (index, slot) in out.iter_mut().enumerate() {
            let product = self.0[index] as u128 * rhs as u128 + carry;
            *slot = product as u64;
            carry = product >> 64;
        }
        if carry != 0 { return Err(KernelError::U256Overflow); }
        Ok(Self(out))
    }
    fn checked_mul_u128(self, rhs: u128) -> Result<Self, KernelError> {
        let low = self.checked_mul_u64(rhs as u64)?;
        let high = self.checked_mul_u64((rhs >> 64) as u64)?.checked_shl_64()?;
        low.checked_add(high)
    }
    fn checked_shl_64(self) -> Result<Self, KernelError> {
        if self.0[3] != 0 { return Err(KernelError::U256Overflow); }
        Ok(Self([0, self.0[0], self.0[1], self.0[2]]))
    }
    fn bit(self, index: usize) -> bool {
        ((self.0[index / 64] >> (index % 64)) & 1) != 0
    }
    fn div_rem_bounded(self, denominator: Self) -> Result<(Self, Self), KernelError> {
        if denominator == Self::ZERO { return Err(KernelError::ZeroDenominator); }
        let mut quotient = Self::ZERO;
        let mut remainder = Self::ZERO;
        for bit in (0..256).rev() {
            if remainder.0[3] >> 63 != 0 { return Err(KernelError::U256Overflow); }
            let mut shifted = [0_u64; 4];
            let mut carry = 0_u64;
            for (index, slot) in shifted.iter_mut().enumerate() {
                let next = remainder.0[index] >> 63;
                *slot = (remainder.0[index] << 1) | carry;
                carry = next;
            }
            remainder = Self(shifted);
            if self.bit(bit) { remainder = remainder.checked_add(Self::ONE)?; }
            if remainder.compare(denominator) != Ordering::Less {
                remainder = remainder.checked_sub(denominator)?;
                quotient.0[bit / 64] |= 1_u64 << (bit % 64);
            }
        }
        Ok((quotient, remainder))
    }
    fn try_u64(self) -> Result<u64, KernelError> {
        if self.0[1..].iter().any(|limb| *limb != 0) { return Err(KernelError::InvalidNarrowing); }
        Ok(self.0[0])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PriceBoundsQ64 { p_lo: PriceQ64, p_hi: PriceQ64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AuthenticatedQuoteCap(QuoteE6);
impl AuthenticatedQuoteCap {
    fn from_authenticated_parent_for_test(value: u64) -> Self { Self(value) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OpenRequirement {
    required_pre_haircut_quote_e6_wide: QuoteE6Wide,
    required_allocation_atoms: CollateralAtoms,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FinalDebitPartition {
    theoretical_debit_atoms_wide: HostU256,
    debited_atoms: CollateralAtoms,
    covered_loss_quote_e6: QuoteE6,
    uncovered_loss_quote_e6: QuoteE6,
}

fn collateral_scale(decimals: u8) -> Result<u64, KernelError> {
    match decimals {
        6 => Ok(1_000_000),
        7 => Ok(10_000_000),
        8 => Ok(100_000_000),
        9 => Ok(1_000_000_000),
        _ => Err(KernelError::InvalidDecimals),
    }
}
fn checked_price_bounds(p_lo: PriceQ64, p_hi: PriceQ64) -> Result<PriceBoundsQ64, KernelError> {
    if p_lo == 0 || p_hi == 0 { return Err(KernelError::ZeroPrice); }
    if p_lo > p_hi { return Err(KernelError::InvertedPriceBounds); }
    Ok(PriceBoundsQ64 { p_lo, p_hi })
}
fn quote_e6_floor_wide(atoms: CollateralAtoms, price_q64: PriceQ64, collateral_decimals: u8) -> Result<QuoteE6Wide, KernelError> {
    let scale = collateral_scale(collateral_decimals)?;
    if price_q64 == 0 { return Err(KernelError::ZeroPrice); }
    let numerator = HostU256::from_u64(atoms)
        .checked_mul_u128(price_q64)?
        .checked_mul_u64(QUOTE_E6_SCALE)?;
    let denominator = HostU256::from_u128(scale as u128 * Q64_ONE);
    Ok(numerator.div_rem_bounded(denominator)?.0)
}
fn atom_pair_wide(quote_e6_wide: QuoteE6Wide, price_q64: PriceQ64, collateral_decimals: u8) -> Result<(HostU256, HostU256, HostU256), KernelError> {
    let scale = collateral_scale(collateral_decimals)?;
    if price_q64 == 0 { return Err(KernelError::ZeroPrice); }
    let factor = HostU256::from_u128(scale as u128 * Q64_ONE);
    let q_max = HostU256::MAX.div_rem_bounded(factor)?.0;
    if quote_e6_wide.compare(q_max) == Ordering::Greater { return Err(KernelError::OutOfEnvelope); }
    let numerator = quote_e6_wide.checked_mul_u64(scale)?.checked_shl_64()?;
    let denominator = HostU256::from_u128(price_q64).checked_mul_u64(QUOTE_E6_SCALE)?;
    let (floor, remainder) = numerator.div_rem_bounded(denominator)?;
    let ceil = if remainder == HostU256::ZERO { floor } else { floor.checked_add(HostU256::ONE)? };
    Ok((floor, ceil, remainder))
}
fn atoms_debit_ceil_wide(quote_e6_wide: QuoteE6Wide, price_q64: PriceQ64, collateral_decimals: u8) -> Result<HostU256, KernelError> {
    Ok(atom_pair_wide(quote_e6_wide, price_q64, collateral_decimals)?.1)
}
fn atoms_payout_floor_wide(quote_e6_wide: QuoteE6Wide, price_q64: PriceQ64, collateral_decimals: u8) -> Result<HostU256, KernelError> {
    Ok(atom_pair_wide(quote_e6_wide, price_q64, collateral_decimals)?.0)
}
fn credited_quote_e6_wide(atoms: CollateralAtoms, p_lo_q64: PriceQ64, collateral_decimals: u8) -> Result<QuoteE6Wide, KernelError> {
    let raw = quote_e6_floor_wide(atoms, p_lo_q64, collateral_decimals)?;
    Ok(raw.checked_mul_u64(COLLATERAL_FACTOR_BPS)?.div_rem_bounded(HostU256::from_u64(BPS_DENOMINATOR))?.0)
}
fn cap_quote_wide(value: QuoteE6Wide, authenticated_cap: AuthenticatedQuoteCap) -> QuoteE6 {
    if value.compare(HostU256::from_u64(authenticated_cap.0)) == Ordering::Greater {
        authenticated_cap.0
    } else {
        value.try_u64().expect("value bounded by authenticated u64 cap")
    }
}
fn open_pre_haircut_requirement(initial_margin_quote_e6: QuoteE6, prices: PriceBoundsQ64, collateral_decimals: u8, free_atoms: CollateralAtoms) -> Result<OpenRequirement, KernelError> {
    collateral_scale(collateral_decimals)?;
    let pre_num = HostU256::from_u64(initial_margin_quote_e6).checked_mul_u64(BPS_DENOMINATOR)?;
    let (pre_q, pre_r) = pre_num.div_rem_bounded(HostU256::from_u64(COLLATERAL_FACTOR_BPS))?;
    let required_pre_haircut_quote_e6_wide = if pre_r == HostU256::ZERO { pre_q } else { pre_q.checked_add(HostU256::ONE)? };
    let required_wide = atoms_debit_ceil_wide(required_pre_haircut_quote_e6_wide, prices.p_lo, collateral_decimals)?;
    let required_allocation_atoms = required_wide.try_u64()?;
    if required_allocation_atoms > free_atoms { return Err(KernelError::InsufficientFreeAtoms); }
    Ok(OpenRequirement { required_pre_haircut_quote_e6_wide, required_allocation_atoms })
}
fn final_debit_partition(loss_quote_e6: QuoteE6, prices: PriceBoundsQ64, collateral_decimals: u8, eligible_atoms: CollateralAtoms) -> Result<FinalDebitPartition, KernelError> {
    collateral_scale(collateral_decimals)?;
    let theoretical_debit_atoms_wide = atoms_debit_ceil_wide(HostU256::from_u64(loss_quote_e6), prices.p_lo, collateral_decimals)?;
    let eligible_wide = HostU256::from_u64(eligible_atoms);
    let debited_atoms = if theoretical_debit_atoms_wide.compare(eligible_wide) == Ordering::Greater {
        eligible_atoms
    } else {
        theoretical_debit_atoms_wide.try_u64()?
    };
    let covered_loss_quote_e6 = if theoretical_debit_atoms_wide.compare(eligible_wide) != Ordering::Greater {
        loss_quote_e6
    } else {
        cap_quote_wide(
            quote_e6_floor_wide(debited_atoms, prices.p_lo, collateral_decimals)?,
            AuthenticatedQuoteCap::from_authenticated_parent_for_test(loss_quote_e6),
        )
    };
    let uncovered_loss_quote_e6 = loss_quote_e6.checked_sub(covered_loss_quote_e6).ok_or(KernelError::U256Underflow)?;
    if debited_atoms > eligible_atoms || covered_loss_quote_e6.checked_add(uncovered_loss_quote_e6) != Some(loss_quote_e6) {
        return Err(KernelError::U256Overflow);
    }
    Ok(FinalDebitPartition { theoretical_debit_atoms_wide, debited_atoms, covered_loss_quote_e6, uncovered_loss_quote_e6 })
}
fn displayed_subatom_impairment_bound_quote_e6(prices: PriceBoundsQ64, collateral_decimals: u8) -> Result<QuoteE6Wide, KernelError> {
    let scale = collateral_scale(collateral_decimals)?;
    let numerator = HostU256::from_u128(prices.p_hi).checked_mul_u64(QUOTE_E6_SCALE)?;
    let denominator = HostU256::from_u128(scale as u128 * Q64_ONE);
    let (quotient, remainder) = numerator.div_rem_bounded(denominator)?;
    if remainder == HostU256::ZERO { Ok(quotient) } else { quotient.checked_add(HostU256::ONE) }
}
fn identity_price_bounds_by_address_equality(collateral_mint: &[u8; 32], quote_mint: &[u8; 32], authenticated_mint_decimals: u8) -> Result<PriceBoundsQ64, KernelError> {
    if collateral_mint.iter().all(|byte| *byte == 0) || quote_mint.iter().all(|byte| *byte == 0) {
        return Err(KernelError::DefaultValueRejected);
    }
    if collateral_mint != quote_mint { return Err(KernelError::IdentityAddressMismatch); }
    if authenticated_mint_decimals != 6 { return Err(KernelError::InvalidDecimals); }
    Ok(PriceBoundsQ64 { p_lo: Q64_ONE, p_hi: Q64_ONE })
}

struct FieldBinding { name: &'static str, json: &'static str }
struct RowBinding {
    family: &'static str,
    id: &'static str,
    operation: Option<&'static str>,
    fields: &'static [FieldBinding],
}
const ROWS: &[RowBinding] = &[
    RowBinding { family: "atoms_floor_ceil", id: "atoms_zero", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"0\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"0\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_identity_u64_max", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"18446744073709551615\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_exact_d7", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"10\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"10\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_exact_d8", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"100\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"100\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_exact_d9", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1000\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"1000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_remainder_one_third", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"18446744073709551616000000\"" },
        FieldBinding { name: "price_q64", json: "\"55340232221128654848\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_remainder_two_thirds", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"36893488147419103232000000\"" },
        FieldBinding { name: "price_q64", json: "\"55340232221128654848\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"2\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_exact_three_thirds", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"55340232221128654848\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"3\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_near_one_low", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1000001\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"1000000\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"1000000000000\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551615\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_near_one_high", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1000000\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"999999\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"18446744073708551617000000\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551617\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_min_price_d9", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"18446744073709551616000\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"18446744073709551616000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_max_price_u64_quote", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"1\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"340282366920938463444927863358058659840000000\"" },
        FieldBinding { name: "price_q64", json: "\"340282366920938463463374607431768211455\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_envelope_max_d6", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"115792089237316195423570985008687907853269984665640554578196335578578944\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"115792089237316195423570985008687907853269984665640554578196335578578944\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"6277101735386680763835789423207666416102355444464034\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_envelope_max_d7", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"115792089237316195423570985008687907853269984665640480791220040740372480\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"115792089237316195423570985008687907853269984665640480791220040740372480\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"627710173538668076383578942320766641610235544446403\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_envelope_max_d8", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"115792089237316195423570985008687907853269984665639927388897829453824000\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"115792089237316195423570985008687907853269984665639927388897829453824000\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"62771017353866807638357894232076664161023554444640\"" },
    ] },
    RowBinding { family: "atoms_floor_ceil", id: "atoms_envelope_max_d9", operation: None, fields: &[
        FieldBinding { name: "atoms_debit_ceil_wide", json: "\"115792089237316195423570985008687907853269984665639927388897829453824000\"" },
        FieldBinding { name: "atoms_payout_floor_wide", json: "\"115792089237316195423570985008687907853269984665639927388897829453824000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "division_remainder", json: "\"0\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"6277101735386680763835789423207666416102355444464\"" },
    ] },
    RowBinding { family: "authenticated_cap", id: "cap_zero", operation: None, fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"0\"" },
        FieldBinding { name: "capped_quote_e6", json: "\"0\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"0\"" },
    ] },
    RowBinding { family: "authenticated_cap", id: "cap_below", operation: None, fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"42\"" },
        FieldBinding { name: "capped_quote_e6", json: "\"41\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"41\"" },
    ] },
    RowBinding { family: "authenticated_cap", id: "cap_equal", operation: None, fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"42\"" },
        FieldBinding { name: "capped_quote_e6", json: "\"42\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"42\"" },
    ] },
    RowBinding { family: "authenticated_cap", id: "cap_above", operation: None, fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"42\"" },
        FieldBinding { name: "capped_quote_e6", json: "\"42\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"43\"" },
    ] },
    RowBinding { family: "authenticated_cap", id: "cap_u256_max", operation: None, fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"18446744073709551615\"" },
        FieldBinding { name: "capped_quote_e6", json: "\"18446744073709551615\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_zero", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"0\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_one_whole_d6", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1000000\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"500000\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_one_whole_d7", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"10000000\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"500000\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_one_whole_d8", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"100000000\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"500000\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_one_whole_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1000000000\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"500000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_half_atom_extinguished", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_odd_floor", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"3\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "credited_quote", id: "credit_max_domain", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "credited_quote_e6_wide", json: "\"170141183460469231722463931679029329919\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_lo_q64", json: "\"340282366920938463463374607431768211455\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_zero", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"0\"" },
        FieldBinding { name: "debited_atoms", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"0\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"0\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"0\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"0\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_eligible_zero", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"0\"" },
        FieldBinding { name: "debited_atoms", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"0\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"1\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"1\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_exact_full", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"1\"" },
        FieldBinding { name: "debited_atoms", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"1\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"1\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"0\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_identity_partial", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"4\"" },
        FieldBinding { name: "debited_atoms", json: "\"4\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"4\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"10\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"10\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"6\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_identity_full_max", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"18446744073709551615\"" },
        FieldBinding { name: "debited_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"18446744073709551615\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"18446744073709551615\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"0\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_theoretical_above_u64", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"0\"" },
        FieldBinding { name: "debited_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "eligible_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"1\"" },
        FieldBinding { name: "p_lo_q64", json: "\"1\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"18446744073709551616000\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"1\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_rounding_partial", operation: None, fields: &[
        FieldBinding { name: "covered_loss_quote_e6", json: "\"0\"" },
        FieldBinding { name: "debited_atoms", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"0\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"3\"" },
        FieldBinding { name: "p_hi_q64", json: "\"55340232221128654848\"" },
        FieldBinding { name: "p_lo_q64", json: "\"55340232221128654848\"" },
        FieldBinding { name: "theoretical_debit_atoms_wide", json: "\"1\"" },
        FieldBinding { name: "uncovered_loss_quote_e6", json: "\"3\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_inverted_bounds", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"1\"" },
        FieldBinding { name: "error", json: "\"INVERTED_PRICE_BOUNDS\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551617\"" },
    ] },
    RowBinding { family: "final_debit_partition", id: "final_zero_price", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "eligible_atoms", json: "\"1\"" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "loss_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"0\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_zero", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "parsed_unsigned", json: "\"0\"" },
        FieldBinding { name: "text", json: "\"0\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_u256_max", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "parsed_unsigned", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_leading_zero", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"01\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_plus", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"+1\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_leading_space", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\" 1\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_trailing_space", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"1 \"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_negative", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"-1\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_empty", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_unsigned_exponent", operation: Some("parse_fixture_unsigned_decimal"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "maximum_inclusive", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "text", json: "\"1e3\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_address_valid_lowercase", operation: Some("parse_fixture_address_hex"), fields: &[
        FieldBinding { name: "address_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "decoded_bytes_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "decoded_length", json: "32" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_address_uppercase", operation: Some("parse_fixture_address_hex"), fields: &[
        FieldBinding { name: "address_hex", json: "\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_address_short", operation: Some("parse_fixture_address_hex"), fields: &[
        FieldBinding { name: "address_hex", json: "\"111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_address_long", operation: Some("parse_fixture_address_hex"), fields: &[
        FieldBinding { name: "address_hex", json: "\"11111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
    ] },
    RowBinding { family: "fixture_parser_cases", id: "parser_address_invalid_tail", operation: Some("parse_fixture_address_hex"), fields: &[
        FieldBinding { name: "address_hex", json: "\"111111111111111111111111111111111111111111111111111111111111111g\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
    ] },
    RowBinding { family: "identity_basis", id: "identity_equal_six_decimals", operation: None, fields: &[
        FieldBinding { name: "authenticated_mint_decimals", json: "6" },
        FieldBinding { name: "collateral_mint_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_mint_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
    ] },
    RowBinding { family: "identity_basis", id: "identity_address_mismatch", operation: None, fields: &[
        FieldBinding { name: "authenticated_mint_decimals", json: "6" },
        FieldBinding { name: "collateral_mint_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "error", json: "\"IDENTITY_ADDRESS_MISMATCH\"" },
        FieldBinding { name: "quote_mint_hex", json: "\"2222222222222222222222222222222222222222222222222222222222222222\"" },
    ] },
    RowBinding { family: "identity_basis", id: "identity_wrong_decimals", operation: None, fields: &[
        FieldBinding { name: "authenticated_mint_decimals", json: "7" },
        FieldBinding { name: "collateral_mint_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "error", json: "\"INVALID_DECIMALS\"" },
        FieldBinding { name: "quote_mint_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
    ] },
    RowBinding { family: "identity_basis", id: "identity_default_zero_rejected", operation: None, fields: &[
        FieldBinding { name: "authenticated_mint_decimals", json: "6" },
        FieldBinding { name: "collateral_mint_hex", json: "\"0000000000000000000000000000000000000000000000000000000000000000\"" },
        FieldBinding { name: "error", json: "\"DEFAULT_VALUE_REJECTED\"" },
        FieldBinding { name: "quote_mint_hex", json: "\"0000000000000000000000000000000000000000000000000000000000000000\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_identity_d6", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_max_price_d6", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"18446744073709551616\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_hi_q64", json: "\"340282366920938463463374607431768211455\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_identity_d7", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_max_price_d7", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1844674407370955162\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "p_hi_q64", json: "\"340282366920938463463374607431768211455\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_identity_d8", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_max_price_d8", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"184467440737095517\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "p_hi_q64", json: "\"340282366920938463463374607431768211455\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_identity_d9", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_max_price_d9", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"18446744073709552\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "p_hi_q64", json: "\"340282366920938463463374607431768211455\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_min_price_d9", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "p_hi_q64", json: "\"1\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_near_one_low_d6", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "impairment_bound", id: "impairment_near_one_high_d6", operation: None, fields: &[
        FieldBinding { name: "bound_quote_e6", json: "\"2\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551617\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_zero", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "free_atoms", json: "\"0\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"0\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "required_allocation_atoms", json: "\"0\"" },
        FieldBinding { name: "required_pre_haircut_quote_e6_wide", json: "\"0\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_identity_exact_fit", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "free_atoms", json: "\"2\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "required_allocation_atoms", json: "\"2\"" },
        FieldBinding { name: "required_pre_haircut_quote_e6_wide", json: "\"2\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_identity_insufficient", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INSUFFICIENT_FREE_ATOMS\"" },
        FieldBinding { name: "free_atoms", json: "\"1\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_largest_identity_fit", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"9223372036854775807\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "required_allocation_atoms", json: "\"18446744073709551614\"" },
        FieldBinding { name: "required_pre_haircut_quote_e6_wide", json: "\"18446744073709551614\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_first_identity_nonfit", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INVALID_NARROWING\"" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"9223372036854775808\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_theoretical_huge", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "error", json: "\"INVALID_NARROWING\"" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"18446744073709551615\"" },
        FieldBinding { name: "p_hi_q64", json: "\"1\"" },
        FieldBinding { name: "p_lo_q64", json: "\"1\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_inverted_bounds", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INVERTED_PRICE_BOUNDS\"" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551617\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_zero_price", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"0\"" },
    ] },
    RowBinding { family: "open_requirement", id: "open_bad_decimals", operation: None, fields: &[
        FieldBinding { name: "decimals", json: "10" },
        FieldBinding { name: "error", json: "\"INVALID_DECIMALS\"" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_zero_d6", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"0\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"0\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_one_whole_d6", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1000000\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_sub_whole_d6", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"999999\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"999999\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_exact_scaled_price_d6", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_one_whole_d7", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"10000000\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_sub_whole_d7", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"9999999\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"999999\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_exact_scaled_price_d7", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "price_q64", json: "\"184467440737095516160\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_one_whole_d8", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"100000000\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_sub_whole_d8", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"99999999\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"999999\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_exact_scaled_price_d8", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "price_q64", json: "\"1844674407370955161600\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_one_whole_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1000000000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_sub_whole_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"999999999\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"999999\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_exact_scaled_price_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616000\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_near_one_low_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1000000000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551615\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"999999\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_near_one_high_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"1000000000\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551617\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"1000000\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_min_price_max_atoms_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"0\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_max_price_max_atoms_d6", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "price_q64", json: "\"340282366920938463463374607431768211455\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"340282366920938463444927863358058659839\"" },
    ] },
    RowBinding { family: "quote_floor", id: "quote_max_price_max_atoms_d9", operation: None, fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "price_q64", json: "\"340282366920938463463374607431768211455\"" },
        FieldBinding { name: "quote_e6_floor_wide", json: "\"340282366920938463444927863358058659\"" },
    ] },
    RowBinding { family: "rejections", id: "quote_zero_price", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "price_q64", json: "\"0\"" },
    ] },
    RowBinding { family: "rejections", id: "quote_decimals_low", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "5" },
        FieldBinding { name: "error", json: "\"INVALID_DECIMALS\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "rejections", id: "quote_decimals_high", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "10" },
        FieldBinding { name: "error", json: "\"INVALID_DECIMALS\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "rejections", id: "atoms_zero_price", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "price_q64", json: "\"0\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "rejections", id: "atoms_out_of_envelope_d6", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"OUT_OF_ENVELOPE\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"6277101735386680763835789423207666416102355444464035\"" },
    ] },
    RowBinding { family: "rejections", id: "atoms_out_of_envelope_d7", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "7" },
        FieldBinding { name: "error", json: "\"OUT_OF_ENVELOPE\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"627710173538668076383578942320766641610235544446404\"" },
    ] },
    RowBinding { family: "rejections", id: "atoms_out_of_envelope_d8", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "8" },
        FieldBinding { name: "error", json: "\"OUT_OF_ENVELOPE\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"62771017353866807638357894232076664161023554444641\"" },
    ] },
    RowBinding { family: "rejections", id: "atoms_out_of_envelope_d9", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "9" },
        FieldBinding { name: "error", json: "\"OUT_OF_ENVELOPE\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"6277101735386680763835789423207666416102355444465\"" },
    ] },
    RowBinding { family: "rejections", id: "impairment_zero_price", operation: Some("displayed_subatom_impairment_bound_quote_e6"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "p_hi_q64", json: "\"0\"" },
    ] },
    RowBinding { family: "rejections", id: "bounds_inverted", operation: Some("checked_price_bounds"), fields: &[
        FieldBinding { name: "error", json: "\"INVERTED_PRICE_BOUNDS\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551617\"" },
    ] },
    RowBinding { family: "rejections", id: "bounds_zero_low", operation: Some("checked_price_bounds"), fields: &[
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"0\"" },
    ] },
    RowBinding { family: "rejections", id: "bounds_zero_high", operation: Some("checked_price_bounds"), fields: &[
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "p_hi_q64", json: "\"0\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_compile_time_brand_cases", id: "ts_brand_reject_raw_bigint", operation: Some("cap_quote_wide"), fields: &[
        FieldBinding { name: "authenticated_cap_expression", json: "\"1n\"" },
        FieldBinding { name: "error", json: "\"TYPESCRIPT_COMPILE_TIME_REJECTION\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "typescript_compile_time_brand_cases", id: "ts_brand_reject_plain_value_object", operation: Some("cap_quote_wide"), fields: &[
        FieldBinding { name: "authenticated_cap_expression", json: "\"{ value: 1n }\"" },
        FieldBinding { name: "error", json: "\"TYPESCRIPT_COMPILE_TIME_REJECTION\"" },
        FieldBinding { name: "value_quote_e6_wide", json: "\"1\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_collateral_atoms_negative", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"-1\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_collateral_atoms_above_u64", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"18446744073709551616\"" },
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_quote_e6_negative", operation: Some("open_pre_haircut_requirement"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "free_atoms", json: "\"0\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"-1\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_quote_e6_above_u64", operation: Some("open_pre_haircut_requirement"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "free_atoms", json: "\"18446744073709551615\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_price_q64_negative", operation: Some("checked_price_bounds"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"-1\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_price_q64_above_u128", operation: Some("checked_price_bounds"), fields: &[
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "p_hi_q64", json: "\"340282366920938463463374607431768211456\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_quote_e6_wide_negative", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"-1\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_quote_e6_wide_above_u256", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639936\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_cap_factory_negative", operation: Some("authenticated_quote_cap_factory"), fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"-1\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_cap_factory_above_u64", operation: Some("authenticated_quote_cap_factory"), fields: &[
        FieldBinding { name: "authenticated_cap_quote_e6", json: "\"18446744073709551616\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_fractional_decimals", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "6.5" },
        FieldBinding { name: "error", json: "\"INVALID_DECIMALS\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_identity_collateral_array_length_31", operation: Some("identity_price_bounds_by_address_equality"), fields: &[
        FieldBinding { name: "authenticated_mint_decimals", json: "6" },
        FieldBinding { name: "collateral_mint_bytes_hex", json: "\"11111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "quote_mint_bytes_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
    ] },
    RowBinding { family: "typescript_runtime_guards", id: "ts_identity_quote_array_length_33", operation: Some("identity_price_bounds_by_address_equality"), fields: &[
        FieldBinding { name: "authenticated_mint_decimals", json: "6" },
        FieldBinding { name: "collateral_mint_bytes_hex", json: "\"1111111111111111111111111111111111111111111111111111111111111111\"" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "quote_mint_bytes_hex", json: "\"111111111111111111111111111111111111111111111111111111111111111111\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_add_overflow", operation: Some("checked_add"), fields: &[
        FieldBinding { name: "error", json: "\"U256_OVERFLOW\"" },
        FieldBinding { name: "left", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "right", json: "\"1\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_mul_overflow", operation: Some("checked_mul_u64"), fields: &[
        FieldBinding { name: "error", json: "\"U256_OVERFLOW\"" },
        FieldBinding { name: "left", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "right", json: "\"2\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_mul_u128_overflow", operation: Some("checked_mul_u128"), fields: &[
        FieldBinding { name: "error", json: "\"U256_OVERFLOW\"" },
        FieldBinding { name: "left", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "right", json: "\"2\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_sub_underflow", operation: Some("checked_sub"), fields: &[
        FieldBinding { name: "error", json: "\"U256_UNDERFLOW\"" },
        FieldBinding { name: "left", json: "\"0\"" },
        FieldBinding { name: "right", json: "\"1\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_narrow_ok", operation: Some("try_u64"), fields: &[
        FieldBinding { name: "u64", json: "\"18446744073709551615\"" },
        FieldBinding { name: "value", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_narrow_reject", operation: Some("try_u64"), fields: &[
        FieldBinding { name: "error", json: "\"INVALID_NARROWING\"" },
        FieldBinding { name: "value", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_divide_by_zero", operation: Some("div_rem_bounded"), fields: &[
        FieldBinding { name: "denominator", json: "\"0\"" },
        FieldBinding { name: "error", json: "\"ZERO_DENOMINATOR\"" },
        FieldBinding { name: "numerator", json: "\"1\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_add_carry_success", operation: Some("checked_add"), fields: &[
        FieldBinding { name: "left", json: "\"18446744073709551615\"" },
        FieldBinding { name: "right", json: "\"1\"" },
        FieldBinding { name: "sum", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_sub_borrow_success", operation: Some("checked_sub"), fields: &[
        FieldBinding { name: "difference", json: "\"18446744073709551615\"" },
        FieldBinding { name: "left", json: "\"18446744073709551616\"" },
        FieldBinding { name: "right", json: "\"1\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_shl_64_success", operation: Some("checked_shl_64"), fields: &[
        FieldBinding { name: "shifted", json: "\"18446744073709551616\"" },
        FieldBinding { name: "value", json: "\"1\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_shl_64_overflow", operation: Some("checked_shl_64"), fields: &[
        FieldBinding { name: "error", json: "\"U256_OVERFLOW\"" },
        FieldBinding { name: "value", json: "\"6277101735386680763835789423207666416102355444464034512896\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_div_high_limb_exact", operation: Some("div_rem_bounded"), fields: &[
        FieldBinding { name: "denominator", json: "\"340282366920938463463374607431768211457\"" },
        FieldBinding { name: "numerator", json: "\"115792089237316195423570985008687907853269984665640564039457584007913129639935\"" },
        FieldBinding { name: "quotient", json: "\"340282366920938463463374607431768211455\"" },
        FieldBinding { name: "remainder", json: "\"0\"" },
    ] },
    RowBinding { family: "u256_primitive_edges", id: "u256_compare_high_limb", operation: Some("compare"), fields: &[
        FieldBinding { name: "comparison", json: "\"greater\"" },
        FieldBinding { name: "left", json: "\"6277101735386680763835789423207666416102355444464034512896\"" },
        FieldBinding { name: "right", json: "\"18446744073709551615\"" },
    ] },
    RowBinding { family: "validation_precedence", id: "precedence_range_before_decimals", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"-1\"" },
        FieldBinding { name: "decimals", json: "5" },
        FieldBinding { name: "error", json: "\"INPUT_OUT_OF_RANGE\"" },
        FieldBinding { name: "price_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "validation_precedence", id: "precedence_decimals_before_price", operation: Some("quote_e6_floor_wide"), fields: &[
        FieldBinding { name: "atoms", json: "\"1\"" },
        FieldBinding { name: "decimals", json: "5" },
        FieldBinding { name: "error", json: "\"INVALID_DECIMALS\"" },
        FieldBinding { name: "price_q64", json: "\"0\"" },
    ] },
    RowBinding { family: "validation_precedence", id: "precedence_price_before_envelope", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "price_q64", json: "\"0\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"6277101735386680763835789423207666416102355444464035\"" },
    ] },
    RowBinding { family: "validation_precedence", id: "precedence_envelope_before_multiplication", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"OUT_OF_ENVELOPE\"" },
        FieldBinding { name: "price_q64", json: "\"1\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"6277101735386680763835789423207666416102355444464035\"" },
    ] },
    RowBinding { family: "validation_precedence", id: "precedence_narrowing_before_availability", operation: Some("open_pre_haircut_requirement"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"INVALID_NARROWING\"" },
        FieldBinding { name: "free_atoms", json: "\"0\"" },
        FieldBinding { name: "initial_margin_quote_e6", json: "\"9223372036854775808\"" },
        FieldBinding { name: "p_hi_q64", json: "\"18446744073709551616\"" },
        FieldBinding { name: "p_lo_q64", json: "\"18446744073709551616\"" },
    ] },
    RowBinding { family: "validation_precedence", id: "precedence_public_zero_price_before_private_denominator", operation: Some("atoms_pair_wide"), fields: &[
        FieldBinding { name: "decimals", json: "6" },
        FieldBinding { name: "error", json: "\"ZERO_PRICE\"" },
        FieldBinding { name: "price_q64", json: "\"0\"" },
        FieldBinding { name: "quote_e6_wide", json: "\"1\"" },
    ] },
];

fn raw_value(json: &'static str) -> &'static str {
    if json.as_bytes().first() == Some(&b'"') {
        &json[1..json.len() - 1]
    } else {
        json
    }
}
fn field<'a>(row: &'a RowBinding, name: &str) -> &'a str {
    raw_value(row.fields.iter().find(|value| value.name == name).unwrap_or_else(|| panic!("{}.{}", row.id, name)).json)
}
fn optional_field<'a>(row: &'a RowBinding, name: &str) -> Option<&'a str> {
    row.fields.iter().find(|value| value.name == name).map(|value| raw_value(value.json))
}
fn parse_fixture_unsigned_decimal(text: &str) -> Result<HostU256, KernelError> {
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(KernelError::InputOutOfRange);
    }
    let mut value = HostU256::ZERO;
    for byte in text.bytes() {
        value = value.checked_mul_u64(10).map_err(|_| KernelError::InputOutOfRange)?;
        value = value.checked_add(HostU256::from_u64((byte - b'0') as u64)).map_err(|_| KernelError::InputOutOfRange)?;
    }
    Ok(value)
}
fn parse_u64(text: &str) -> Result<u64, KernelError> {
    parse_fixture_unsigned_decimal(text)?.try_u64().map_err(|_| KernelError::InputOutOfRange)
}
fn parse_u128(text: &str) -> Result<u128, KernelError> {
    let value = parse_fixture_unsigned_decimal(text)?;
    if value.0[2] != 0 || value.0[3] != 0 { return Err(KernelError::InputOutOfRange); }
    Ok(value.0[0] as u128 | ((value.0[1] as u128) << 64))
}
fn parse_decimals(text: &str) -> Result<u8, KernelError> {
    text.parse::<u8>().map_err(|_| KernelError::InvalidDecimals)
}
fn parse_fixture_address_hex(text: &str) -> Result<[u8; 32], KernelError> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err(KernelError::InputOutOfRange);
    }
    let nibble = |byte: u8| -> u8 { if byte.is_ascii_digit() { byte - b'0' } else { byte - b'a' + 10 } };
    let bytes = text.as_bytes();
    let mut output = [0_u8; 32];
    for index in 0..32 { output[index] = (nibble(bytes[index * 2]) << 4) | nibble(bytes[index * 2 + 1]); }
    Ok(output)
}
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}
fn authenticated_fixture() -> &'static str {
    assert_eq!(to_hex(&hash(FIXTURE).to_bytes()), FIXTURE_SHA256);
    core::str::from_utf8(FIXTURE).unwrap()
}
fn bind_all_rows() {
    let raw = authenticated_fixture();
    assert!(raw.contains("\"marker\": \"COLLATERAL_S00P_CONVERSION_KERNEL_R2_HOST_ONLY\""));
    for claim in ["\"runtime_safety\": false", "\"s01_ready\": false", "\"abi_reserved\": false", "\"release_ready\": false"] {
        assert!(raw.contains(claim));
    }
    assert_eq!(ROWS.len(), 140);
    for row in ROWS {
        let id_needle = format!("\"id\": \"{}\"", row.id);
        assert_eq!(raw.matches(&id_needle).count(), 1, "{}", row.id);
        let position = raw.find(&id_needle).unwrap();
        let start = raw[..position].rfind("\n      {").unwrap() + 7;
        let end = position + raw[position..].find("\n      }").unwrap() + 8;
        let object = &raw[start..end];
        assert!(object.contains(&id_needle), "{}", row.id);
        if let Some(operation) = row.operation {
            assert!(object.contains(&format!("\"operation\": \"{}\"", operation)), "{}", row.id);
        }
        for binding in row.fields {
            assert!(object.contains(&format!("\"{}\": {}", binding.name, binding.json)), "{}.{}", row.id, binding.name);
        }
    }
}
fn assert_error<T>(result: Result<T, KernelError>, expected: &str, id: &str) {
    assert_eq!(result.err().unwrap_or_else(|| panic!("{id} unexpectedly succeeded")).code(), expected, "{id}");
}
fn checked_prices(row: &RowBinding) -> Result<PriceBoundsQ64, KernelError> {
    checked_price_bounds(parse_u128(field(row, "p_lo_q64"))?, parse_u128(field(row, "p_hi_q64"))?)
}

#[test]
fn raw_hash_metadata_and_every_literal_field_are_bound_before_semantics() {
    bind_all_rows();
    let counts = [
        ("quote_floor", 18), ("atoms_floor_ceil", 16), ("credited_quote", 8), ("impairment_bound", 11),
        ("authenticated_cap", 5), ("open_requirement", 9), ("final_debit_partition", 9), ("identity_basis", 4),
        ("rejections", 12), ("u256_primitive_edges", 13), ("typescript_runtime_guards", 13),
        ("typescript_compile_time_brand_cases", 2), ("fixture_parser_cases", 14), ("validation_precedence", 6),
    ];
    for (family, count) in counts { assert_eq!(ROWS.iter().filter(|row| row.family == family).count(), count); }
}

#[test]
fn executes_all_applicable_literal_kernel_rows() {
    bind_all_rows();
    for row in ROWS {
        match row.family {
            "quote_floor" => {
                let actual = quote_e6_floor_wide(parse_u64(field(row, "atoms")).unwrap(), parse_u128(field(row, "price_q64")).unwrap(), parse_decimals(field(row, "decimals")).unwrap()).unwrap();
                assert_eq!(actual, parse_fixture_unsigned_decimal(field(row, "quote_e6_floor_wide")).unwrap(), "{}", row.id);
            }
            "atoms_floor_ceil" => {
                let actual = atom_pair_wide(parse_fixture_unsigned_decimal(field(row, "quote_e6_wide")).unwrap(), parse_u128(field(row, "price_q64")).unwrap(), parse_decimals(field(row, "decimals")).unwrap()).unwrap();
                assert_eq!(actual.0, parse_fixture_unsigned_decimal(field(row, "atoms_payout_floor_wide")).unwrap(), "{}", row.id);
                assert_eq!(actual.1, parse_fixture_unsigned_decimal(field(row, "atoms_debit_ceil_wide")).unwrap(), "{}", row.id);
                assert_eq!(actual.2, parse_fixture_unsigned_decimal(field(row, "division_remainder")).unwrap(), "{}", row.id);
            }
            "credited_quote" => {
                let actual = credited_quote_e6_wide(parse_u64(field(row, "atoms")).unwrap(), parse_u128(field(row, "p_lo_q64")).unwrap(), parse_decimals(field(row, "decimals")).unwrap()).unwrap();
                assert_eq!(actual, parse_fixture_unsigned_decimal(field(row, "credited_quote_e6_wide")).unwrap(), "{}", row.id);
            }
            "impairment_bound" => {
                let price = parse_u128(field(row, "p_hi_q64")).unwrap();
                let actual = displayed_subatom_impairment_bound_quote_e6(checked_price_bounds(price, price).unwrap(), parse_decimals(field(row, "decimals")).unwrap()).unwrap();
                assert_eq!(actual, parse_fixture_unsigned_decimal(field(row, "bound_quote_e6")).unwrap(), "{}", row.id);
            }
            "authenticated_cap" => {
                let actual = cap_quote_wide(parse_fixture_unsigned_decimal(field(row, "value_quote_e6_wide")).unwrap(), AuthenticatedQuoteCap::from_authenticated_parent_for_test(parse_u64(field(row, "authenticated_cap_quote_e6")).unwrap()));
                assert_eq!(actual, parse_u64(field(row, "capped_quote_e6")).unwrap(), "{}", row.id);
            }
            "open_requirement" => {
                let result = checked_prices(row).and_then(|prices| open_pre_haircut_requirement(
                    parse_u64(field(row, "initial_margin_quote_e6")).unwrap(), prices,
                    parse_decimals(field(row, "decimals")).unwrap(), parse_u64(field(row, "free_atoms")).unwrap()));
                if let Some(error) = optional_field(row, "error") { assert_error(result, error, row.id); } else {
                    let actual = result.unwrap();
                    assert_eq!(actual.required_pre_haircut_quote_e6_wide, parse_fixture_unsigned_decimal(field(row, "required_pre_haircut_quote_e6_wide")).unwrap(), "{}", row.id);
                    assert_eq!(actual.required_allocation_atoms, parse_u64(field(row, "required_allocation_atoms")).unwrap(), "{}", row.id);
                }
            }
            "final_debit_partition" => {
                let result = checked_prices(row).and_then(|prices| final_debit_partition(
                    parse_u64(field(row, "loss_quote_e6")).unwrap(), prices,
                    parse_decimals(field(row, "decimals")).unwrap(), parse_u64(field(row, "eligible_atoms")).unwrap()));
                if let Some(error) = optional_field(row, "error") { assert_error(result, error, row.id); } else {
                    let actual = result.unwrap();
                    assert_eq!(actual.theoretical_debit_atoms_wide, parse_fixture_unsigned_decimal(field(row, "theoretical_debit_atoms_wide")).unwrap(), "{}", row.id);
                    assert_eq!(actual.debited_atoms, parse_u64(field(row, "debited_atoms")).unwrap(), "{}", row.id);
                    assert_eq!(actual.covered_loss_quote_e6, parse_u64(field(row, "covered_loss_quote_e6")).unwrap(), "{}", row.id);
                    assert_eq!(actual.uncovered_loss_quote_e6, parse_u64(field(row, "uncovered_loss_quote_e6")).unwrap(), "{}", row.id);
                }
            }
            "identity_basis" => {
                let collateral = parse_fixture_address_hex(field(row, "collateral_mint_hex")).unwrap();
                let quote = parse_fixture_address_hex(field(row, "quote_mint_hex")).unwrap();
                let result = identity_price_bounds_by_address_equality(&collateral, &quote, parse_decimals(field(row, "authenticated_mint_decimals")).unwrap());
                if let Some(error) = optional_field(row, "error") { assert_error(result, error, row.id); } else {
                    let actual = result.unwrap();
                    assert_eq!(actual.p_lo, parse_u128(field(row, "p_lo_q64")).unwrap());
                    assert_eq!(actual.p_hi, parse_u128(field(row, "p_hi_q64")).unwrap());
                }
            }
            _ => {}
        }
    }
}

#[test]
fn executes_generic_rejections_and_validation_precedence() {
    bind_all_rows();
    for row in ROWS.iter().filter(|row| row.family == "rejections") {
        let result = match row.operation.unwrap() {
            "quote_e6_floor_wide" => quote_e6_floor_wide(parse_u64(field(row, "atoms")).unwrap(), parse_u128(field(row, "price_q64")).unwrap(), parse_decimals(field(row, "decimals")).unwrap()).map(|_| ()),
            "atoms_pair_wide" => atom_pair_wide(parse_fixture_unsigned_decimal(field(row, "quote_e6_wide")).unwrap(), parse_u128(field(row, "price_q64")).unwrap(), parse_decimals(field(row, "decimals")).unwrap()).map(|_| ()),
            "displayed_subatom_impairment_bound_quote_e6" => {
                let price = parse_u128(field(row, "p_hi_q64")).unwrap();
                checked_price_bounds(price, price).and_then(|prices| displayed_subatom_impairment_bound_quote_e6(prices, parse_decimals(field(row, "decimals")).unwrap())).map(|_| ())
            }
            "checked_price_bounds" => checked_price_bounds(parse_u128(field(row, "p_lo_q64")).unwrap(), parse_u128(field(row, "p_hi_q64")).unwrap()).map(|_| ()),
            operation => panic!("unknown {operation}"),
        };
        assert_error(result, field(row, "error"), row.id);
    }
    for row in ROWS.iter().filter(|row| row.family == "validation_precedence") {
        let result = match row.id {
            "precedence_range_before_decimals" => parse_u64(field(row, "atoms")).map(|_| ()),
            "precedence_decimals_before_price" => quote_e6_floor_wide(1, 0, 5).map(|_| ()),
            "precedence_price_before_envelope" => atom_pair_wide(parse_fixture_unsigned_decimal(field(row, "quote_e6_wide")).unwrap(), 0, 6).map(|_| ()),
            "precedence_envelope_before_multiplication" => atom_pair_wide(parse_fixture_unsigned_decimal(field(row, "quote_e6_wide")).unwrap(), 1, 6).map(|_| ()),
            "precedence_narrowing_before_availability" => open_pre_haircut_requirement(
                parse_u64(field(row, "initial_margin_quote_e6")).unwrap(), checked_prices(row).unwrap(), 6, 0).map(|_| ()),
            "precedence_public_zero_price_before_private_denominator" => atom_pair_wide(HostU256::ONE, 0, 6).map(|_| ()),
            id => panic!("unknown {id}"),
        };
        assert_error(result, field(row, "error"), row.id);
    }
}

#[test]
fn executes_all_primitive_and_canonical_parser_rows() {
    bind_all_rows();
    for row in ROWS.iter().filter(|row| row.family == "u256_primitive_edges") {
        let expected_error = optional_field(row, "error");
        let operation = row.operation.unwrap();
        let result: Result<(), KernelError> = match operation {
            "checked_add" => {
                let value = parse_fixture_unsigned_decimal(field(row, "left")).unwrap().checked_add(parse_fixture_unsigned_decimal(field(row, "right")).unwrap());
                if let Some(expected) = optional_field(row, "sum") { value.map(|actual| assert_eq!(actual, parse_fixture_unsigned_decimal(expected).unwrap(), "{}", row.id)) } else { value.map(|_| ()) }
            }
            "checked_sub" => {
                let value = parse_fixture_unsigned_decimal(field(row, "left")).unwrap().checked_sub(parse_fixture_unsigned_decimal(field(row, "right")).unwrap());
                if let Some(expected) = optional_field(row, "difference") { value.map(|actual| assert_eq!(actual, parse_fixture_unsigned_decimal(expected).unwrap(), "{}", row.id)) } else { value.map(|_| ()) }
            }
            "checked_mul_u64" => parse_fixture_unsigned_decimal(field(row, "left")).unwrap().checked_mul_u64(parse_u64(field(row, "right")).unwrap()).map(|_| ()),
            "checked_mul_u128" => parse_fixture_unsigned_decimal(field(row, "left")).unwrap().checked_mul_u128(parse_u128(field(row, "right")).unwrap()).map(|_| ()),
            "checked_shl_64" => {
                let value = parse_fixture_unsigned_decimal(field(row, "value")).unwrap().checked_shl_64();
                if let Some(expected) = optional_field(row, "shifted") { value.map(|actual| assert_eq!(actual, parse_fixture_unsigned_decimal(expected).unwrap(), "{}", row.id)) } else { value.map(|_| ()) }
            }
            "try_u64" => {
                let value = parse_fixture_unsigned_decimal(field(row, "value")).unwrap().try_u64();
                if let Some(expected) = optional_field(row, "u64") { value.map(|actual| assert_eq!(actual, parse_u64(expected).unwrap(), "{}", row.id)) } else { value.map(|_| ()) }
            }
            "div_rem_bounded" => {
                let value = parse_fixture_unsigned_decimal(field(row, "numerator")).unwrap().div_rem_bounded(parse_fixture_unsigned_decimal(field(row, "denominator")).unwrap());
                if optional_field(row, "quotient").is_some() {
                    value.map(|actual| {
                        assert_eq!(actual.0, parse_fixture_unsigned_decimal(field(row, "quotient")).unwrap(), "{}", row.id);
                        assert_eq!(actual.1, parse_fixture_unsigned_decimal(field(row, "remainder")).unwrap(), "{}", row.id);
                    })
                } else { value.map(|_| ()) }
            }
            "compare" => {
                let actual = parse_fixture_unsigned_decimal(field(row, "left")).unwrap().compare(parse_fixture_unsigned_decimal(field(row, "right")).unwrap());
                assert_eq!(if actual == Ordering::Greater { "greater" } else { "not_greater" }, field(row, "comparison"));
                Ok(())
            }
            _ => panic!("unknown {operation}"),
        };
        if let Some(error) = expected_error { assert_error(result, error, row.id); } else { result.unwrap(); }
    }
    for row in ROWS.iter().filter(|row| row.family == "fixture_parser_cases") {
        let result: Result<(), KernelError> = match row.operation.unwrap() {
            "parse_fixture_unsigned_decimal" => parse_fixture_unsigned_decimal(field(row, "text")).map(|actual| {
                assert_eq!(actual, parse_fixture_unsigned_decimal(field(row, "parsed_unsigned")).unwrap(), "{}", row.id);
                assert!(actual.compare(parse_fixture_unsigned_decimal(field(row, "maximum_inclusive")).unwrap()) != Ordering::Greater);
            }),
            "parse_fixture_address_hex" => parse_fixture_address_hex(field(row, "address_hex")).map(|actual| {
                assert_eq!(actual.len(), parse_u64(field(row, "decoded_length")).unwrap() as usize);
                assert_eq!(to_hex(&actual), field(row, "decoded_bytes_hex"));
            }),
            operation => panic!("unknown {operation}"),
        };
        if let Some(error) = optional_field(row, "error") { assert_error(result, error, row.id); } else { result.unwrap(); }
    }
}

#[test]
fn executes_both_exact_168_case_grids_and_envelope_boundaries() {
    bind_all_rows();
    let decimals = [6_u8, 7, 8, 9];
    let atoms = [0_u64, 1, 2, 999_999, 1_000_000, u64::MAX - 1, u64::MAX];
    let quotes = [0_u64, 1, 2, 999_999, 1_000_000, u64::MAX - 1, u64::MAX];
    let prices = [1_u128, Q64_ONE - 1, Q64_ONE, Q64_ONE + 1, u128::MAX - 1, u128::MAX];
    let mut quote_cases = 0;
    let mut atom_cases = 0;
    for decimals_value in decimals {
        for price in prices {
            let mut previous_quote = HostU256::ZERO;
            let mut previous_floor = HostU256::ZERO;
            let mut previous_ceil = HostU256::ZERO;
            for atoms_value in atoms {
                let value = quote_e6_floor_wide(atoms_value, price, decimals_value).unwrap();
                assert!(value.compare(previous_quote) != Ordering::Less);
                previous_quote = value;
                quote_cases += 1;
            }
            for quote in quotes {
                let (floor, ceil, remainder) = atom_pair_wide(HostU256::from_u64(quote), price, decimals_value).unwrap();
                assert!(floor.compare(previous_floor) != Ordering::Less);
                assert!(ceil.compare(previous_ceil) != Ordering::Less);
                previous_floor = floor; previous_ceil = ceil;
                assert!(ceil == floor || ceil == floor.checked_add(HostU256::ONE).unwrap());
                let scale = collateral_scale(decimals_value).unwrap();
                let numerator = HostU256::from_u64(quote).checked_mul_u64(scale).unwrap().checked_shl_64().unwrap();
                let denominator = HostU256::from_u128(price).checked_mul_u64(QUOTE_E6_SCALE).unwrap();
                assert_eq!(numerator, floor.checked_mul_u128(price).unwrap().checked_mul_u64(QUOTE_E6_SCALE).unwrap().checked_add(remainder).unwrap());
                assert!(remainder.compare(denominator) == Ordering::Less);
                if floor.compare(HostU256::from_u64(u64::MAX)) != Ordering::Greater {
                    assert!(quote_e6_floor_wide(floor.try_u64().unwrap(), price, decimals_value).unwrap().compare(HostU256::from_u64(quote)) != Ordering::Greater);
                }
                if ceil.compare(HostU256::from_u64(u64::MAX)) != Ordering::Greater {
                    assert!(quote_e6_floor_wide(ceil.try_u64().unwrap(), price, decimals_value).unwrap().compare(HostU256::from_u64(quote)) != Ordering::Less);
                }
                atom_cases += 1;
            }
        }
    }
    assert_eq!(quote_cases, 168);
    assert_eq!(atom_cases, 168);
    for decimals_value in decimals {
        for atoms_value in atoms {
            let mut previous = HostU256::ZERO;
            for price in prices {
                let value = quote_e6_floor_wide(atoms_value, price, decimals_value).unwrap();
                assert!(value.compare(previous) != Ordering::Less);
                previous = value;
            }
        }
    }
    for decimals_value in decimals {
        for quote in quotes {
            let mut previous_floor = None;
            let mut previous_ceil = None;
            for price in prices {
                let (floor, ceil, _) = atom_pair_wide(HostU256::from_u64(quote), price, decimals_value).unwrap();
                if let Some(prior) = previous_floor { assert!(floor.compare(prior) != Ordering::Greater); }
                if let Some(prior) = previous_ceil { assert!(ceil.compare(prior) != Ordering::Greater); }
                previous_floor = Some(floor); previous_ceil = Some(ceil);
            }
        }
    }
    for atoms_value in atoms {
        for price in prices {
            let mut previous = None;
            for decimals_value in decimals {
                let value = quote_e6_floor_wide(atoms_value, price, decimals_value).unwrap();
                if let Some(prior) = previous { assert!(value.compare(prior) != Ordering::Greater); }
                previous = Some(value);
            }
        }
    }
    for quote in quotes {
        for price in prices {
            let mut previous_floor = None;
            let mut previous_ceil = None;
            for decimals_value in decimals {
                let (floor, ceil, _) = atom_pair_wide(HostU256::from_u64(quote), price, decimals_value).unwrap();
                if let Some(prior) = previous_floor { assert!(floor.compare(prior) != Ordering::Less); }
                if let Some(prior) = previous_ceil { assert!(ceil.compare(prior) != Ordering::Less); }
                previous_floor = Some(floor); previous_ceil = Some(ceil);
            }
        }
    }
    assert_eq!(quote_e6_floor_wide(u64::MAX, Q64_ONE, 6).unwrap(), HostU256::from_u64(u64::MAX));
    assert_eq!(atoms_payout_floor_wide(HostU256::from_u64(u64::MAX), Q64_ONE, 6).unwrap(), HostU256::from_u64(u64::MAX));
    for decimals_value in decimals {
        let scale = HostU256::from_u128(collateral_scale(decimals_value).unwrap() as u128 * Q64_ONE);
        let q_max = HostU256::MAX.div_rem_bounded(scale).unwrap().0;
        atom_pair_wide(q_max, 1, decimals_value).unwrap();
        assert_error(atom_pair_wide(q_max.checked_add(HostU256::ONE).unwrap(), 1, decimals_value), "OUT_OF_ENVELOPE", "q_max+1");
    }
}

#[test]
fn private_bounds_and_open_final_asymmetry_are_explicit() {
    let prices = checked_price_bounds(Q64_ONE, Q64_ONE).unwrap();
    assert_eq!(open_pre_haircut_requirement(1, prices, 6, 2).unwrap().required_allocation_atoms, 2);
    assert_error(open_pre_haircut_requirement(1, prices, 6, 1), "INSUFFICIENT_FREE_ATOMS", "open-no-clamp");
    assert_error(open_pre_haircut_requirement(1_u64 << 63, prices, 6, 0), "INVALID_NARROWING", "open-fit-before-free");
    let final_value = final_debit_partition(1, checked_price_bounds(1, 1).unwrap(), 9, u64::MAX).unwrap();
    assert!(final_value.theoretical_debit_atoms_wide.compare(HostU256::from_u64(u64::MAX)) == Ordering::Greater);
    assert_eq!(final_value.debited_atoms, u64::MAX);
    assert_eq!(final_value.covered_loss_quote_e6 + final_value.uncovered_loss_quote_e6, 1);
    assert_eq!(HostU256::from_limbs_le([1, 2, 3, 4]), HostU256([1, 2, 3, 4]));
}
