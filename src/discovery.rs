//! Pure depth caps and support arithmetic. Inputs carry no authentication.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryError {
    InvalidInput,
    Overflow,
    InvalidConfig,
    BoundsExceeded,
}

fn bad() -> DiscoveryError {
    DiscoveryError::InvalidInput
}

const POINT_CAP_PARAMS: [u16; 4] = [3, 50, 3, 10];
pub const DEPTH_MULTIPLE_BPS: u128 = 10_000;
pub const SINGLE_ROW_MULTIPLE_BPS: u128 = 2_500;
pub const DEPTH_BUDGET_CAP: u128 = 50_000_000_000;

pub fn live_point_cap(depth_usdc: u128) -> u128 {
    depth_usdc.saturating_mul(u128::from(POINT_CAP_PARAMS[0]))
}
pub fn live_point_market_cap(depth_usdc: u128) -> u128 {
    depth_usdc.saturating_mul(u128::from(POINT_CAP_PARAMS[3]))
}

/// Caller supplies a depth floor in quote units with six decimals.
pub fn sampled_caps(floor_usdc: u128) -> (u128, u128) {
    (
        floor_usdc.saturating_mul(1_000) / 10_000,
        floor_usdc.saturating_mul(2_500) / 10_000,
    )
}

pub fn secure_caps(floor_usdc: u128) -> (u128, u128) {
    (floor_usdc.saturating_mul(25), floor_usdc.saturating_mul(35))
}

pub fn point_caps(floor_usdc: u128, secure_lift: bool) -> (u128, u128) {
    if secure_lift {
        secure_caps(floor_usdc)
    } else {
        sampled_caps(floor_usdc)
    }
}

/// floor(F * lowerQ64 * 1e6 / (10^decimals * 2^64)); USDC6 passes F unchanged.
pub fn sampled_floor_usdc(
    floor: u128,
    lower_x64: u128,
    quote_decimals: u8,
) -> Result<u128, DiscoveryError> {
    if quote_decimals > 18 || lower_x64 == 0 {
        return Err(bad());
    }
    let mut wide = [0u64; 5];
    for (i, a) in [floor as u64, (floor >> 64) as u64].into_iter().enumerate() {
        let mut carry = 0u128;
        for (j, b) in [lower_x64 as u64, (lower_x64 >> 64) as u64]
            .into_iter()
            .enumerate()
        {
            let v = u128::from(a) * u128::from(b) + u128::from(wide[i + j]) + carry;
            wide[i + j] = v as u64;
            carry = v >> 64;
        }
        wide[i + 2] = carry as u64;
    }
    let mut carry = 0u128;
    for limb in &mut wide {
        let v = u128::from(*limb) * 1_000_000 + carry;
        *limb = v as u64;
        carry = v >> 64;
    }
    if carry != 0 || wide[4] != 0 {
        return Err(bad());
    }
    let divisor = 10u128.pow(u32::from(quote_decimals));
    let mut rem = 0u128;
    for limb in wide.iter_mut().rev() {
        let v = (rem << 64) | u128::from(*limb);
        *limb = (v / divisor) as u64;
        rem = v % divisor;
    }
    if wide[3] != 0 || wide[4] != 0 {
        return Err(bad());
    }
    Ok(u128::from(wide[1]) | (u128::from(wide[2]) << 64))
}

pub type Support = Option<(u32, u32)>;
/// Type tag of a current-point leg: it earns no interval, `overlap` excludes it and `meet` retains the raw endpoint intersection.
pub const POINT: Support = Some((u32::MAX, u32::MAX));
pub fn span(start: i64, end: i64) -> Result<(u32, u32), DiscoveryError> {
    Ok((
        u32::try_from(start).map_err(|_| bad())?,
        u32::try_from(end).map_err(|_| bad())?,
    ))
}
pub fn meet(a: Support, b: Support) -> Support {
    match (a, b) {
        (Some((s0, e0)), Some((s1, e1))) => Some((s0.max(s1), e0.min(e1))),
        (a, None) => a,
        (None, b) => b,
    }
}
/// Common support of priced legs; with any sampled leg, every leg must be supported and all must share >=120 s.
#[inline(never)]
pub fn overlap(legs: &[(Support, bool)]) -> Result<Support, DiscoveryError> {
    let real = || legs.iter().filter(|(support, _)| *support != POINT);
    let n = real().count();
    if n == 0 && !legs.is_empty() {
        return Ok(POINT);
    }
    let sampled = (n > 1 || n < legs.len()) && real().any(|(_, sampled)| *sampled);
    if real().any(|(support, _)| support.is_none()) {
        if sampled {
            return Err(bad());
        }
        return Ok(None);
    }
    let common = real().fold(None, |acc, (support, _)| meet(acc, *support));
    if sampled
        && common.is_none_or(|(start, end)| end.checked_sub(start).is_none_or(|span| span < 120))
    {
        return Err(bad());
    }
    Ok(common)
}

/// Budget multiple: a quarter of the formula for a single contributing row.
pub fn depth_multiple_bps(rows: usize) -> u128 {
    if rows == 1 {
        SINGLE_ROW_MULTIPLE_BPS
    } else {
        DEPTH_MULTIPLE_BPS
    }
}

/// `min(DEPTH_BUDGET_CAP, depth × multiple / 10_000)`.
pub fn depth_budget(depth_usd_e6: u128, multiple_bps: u128) -> Result<u128, DiscoveryError> {
    Ok((depth_usd_e6.checked_mul(multiple_bps).ok_or_else(bad)? / 10_000).min(DEPTH_BUDGET_CAP))
}

/// Caller supplies the economic floor, deviation in basis points, and safety factor.
pub fn floor_cap(
    floor: u128,
    max_oracle_deviation_bps: u16,
    cap_safety_factor: u16,
) -> Result<u128, DiscoveryError> {
    if floor == 0 || max_oracle_deviation_bps == 0 || cap_safety_factor == 0 {
        return Err(DiscoveryError::InvalidInput);
    }
    let band_scaled = floor
        .checked_mul(10_000)
        .ok_or(DiscoveryError::Overflow)?
        .checked_div(u128::from(max_oracle_deviation_bps))
        .ok_or(DiscoveryError::Overflow)?;
    band_scaled
        .checked_div(u128::from(cap_safety_factor))
        .ok_or(DiscoveryError::Overflow)
}

const MAX_POSITION_NOTIONAL: u64 = 1 << 44;

#[inline(always)]
pub fn closeout_notional_cap(
    max_trader_open_interest_bps: u16,
    liquidity_basis: u64,
) -> Result<u64, DiscoveryError> {
    let cap = u128::from(liquidity_basis)
        .checked_mul(u128::from(max_trader_open_interest_bps))
        .ok_or(DiscoveryError::Overflow)?
        / 10_000;
    if cap == 0 {
        return Err(DiscoveryError::InvalidConfig);
    }
    u64::try_from(cap).map_err(|_| DiscoveryError::Overflow)
}

#[inline(always)]
pub fn validate_live_notional(
    max_trader_open_interest_bps: u16,
    free_equity: u64,
    kappa_vault_bps: u16,
    max_notional_policy: u128,
    post_abs_notional: u64,
) -> Result<(), DiscoveryError> {
    let r_vault = u128::from(free_equity)
        .checked_mul(u128::from(kappa_vault_bps))
        .ok_or(DiscoveryError::Overflow)?
        / 10_000;
    let closeout_cap = u128::from(closeout_notional_cap(
        max_trader_open_interest_bps,
        free_equity,
    )?);
    let r_vault_live = r_vault.min(closeout_cap);
    let post_cap = u128::from(MAX_POSITION_NOTIONAL)
        .min(max_notional_policy)
        .min(r_vault_live);
    if post_cap == 0 || u128::from(post_abs_notional) > post_cap {
        return Err(DiscoveryError::BoundsExceeded);
    }
    Ok(())
}
