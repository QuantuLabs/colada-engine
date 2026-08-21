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

    const fn from_u128(value: u128) -> Self {
        Self([value as u64, (value >> 64) as u64, 0, 0])
    }

    #[cfg(test)]
    const fn from_limbs_le(value: [u64; 4]) -> Self {
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

    fn checked_add(self, rhs: Self) -> Result<Self, MathError> {
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

    fn checked_mul_u64(self, rhs: u64) -> Result<Self, MathError> {
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
        let mut quotient = Self::ZERO;
        let mut remainder = Self::ZERO;
        for bit in (0..256).rev() {
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

    fn div_ceil_bounded(self, denominator: Self) -> Result<Self, MathError> {
        let (quotient, remainder) = self.div_rem_bounded(denominator)?;
        if remainder == Self::ZERO {
            Ok(quotient)
        } else {
            quotient.checked_add(Self::ONE)
        }
    }

    fn try_u64(self) -> Result<u64, MathError> {
        if self.0[1] != 0 || self.0[2] != 0 || self.0[3] != 0 {
            return Err(MathError::InvalidNarrowing);
        }
        Ok(self.0[0])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PriceBoundsQ64 {
    p_lo: PriceQ64,
    p_hi: PriceQ64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthenticatedQuoteCap(QuoteE6);

impl AuthenticatedQuoteCap {
    pub(super) const fn from_authenticated_parent(value: QuoteE6) -> Self {
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
    let factor = U256::from_u64(scale).checked_shl_64()?;
    let q_max = U256::MAX.div_rem_bounded(factor)?.0;
    if quote_e6_wide.compare(q_max) == Ordering::Greater {
        return Err(MathError::OutOfEnvelope);
    }
    let numerator = quote_e6_wide.checked_mul_u64(scale)?.checked_shl_64()?;
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
}
