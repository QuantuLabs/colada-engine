#[inline(always)]
pub fn initial_margin_requirement(
    notional: u64,
    initial_margin_bps: u16,
) -> Result<u64, ProgramError> {
    margin_requirement_ceil(notional, initial_margin_bps)
}

#[inline(always)]
fn margin_requirement_ceil(notional: u64, margin_bps: u16) -> Result<u64, ProgramError> {
    let numerator = u128::from(notional)
        .checked_mul(u128::from(margin_bps))
        .ok_or(ProgramError::from(ColadaError::Overflow))?;
    let rounded = numerator
        .checked_add(9_999)
        .ok_or(ProgramError::from(ColadaError::Overflow))?
        / 10_000;
    u64::try_from(rounded).map_err(|_| ProgramError::from(ColadaError::Overflow))
}

const TIER0_INITIAL_MARGIN_BPS: u16 = 500;

const TIER0_MAINTENANCE_MARGIN_BPS: u16 = 250;

const TIER1_INITIAL_MARGIN_BPS: u16 = 200;

const TIER1_MAINTENANCE_MARGIN_BPS: u16 = 100;

const TIER1_TEMPLATE_VERSION_MIN: u16 = 1;

const TIER1_MAX_STALENESS_SECS: i64 = 2;

const TIER1_MAX_CONFIDENCE_BPS: u16 = 100;

#[inline(always)]
fn is_margin_pair_admissible(
    initial_margin_bps: u16,
    maintenance_margin_bps: u16,
    oracle_enabled: bool,
    max_staleness_secs: i64,
    max_confidence_bps: u16,
    template_version: u16,
) -> bool {
    if initial_margin_bps > 10_000 || maintenance_margin_bps >= initial_margin_bps {
        return false;
    }
    if initial_margin_bps == TIER0_INITIAL_MARGIN_BPS
        && maintenance_margin_bps == TIER0_MAINTENANCE_MARGIN_BPS
    {
        return true;
    }
    template_version >= TIER1_TEMPLATE_VERSION_MIN
        && oracle_enabled
        && max_staleness_secs > 0
        && max_staleness_secs <= TIER1_MAX_STALENESS_SECS
        && max_confidence_bps > 0
        && max_confidence_bps <= TIER1_MAX_CONFIDENCE_BPS
        && initial_margin_bps == TIER1_INITIAL_MARGIN_BPS
        && maintenance_margin_bps == TIER1_MAINTENANCE_MARGIN_BPS
}

#[inline(always)]
fn margin_pair_admissible(
    initial_margin_bps: u16,
    maintenance_margin_bps: u16,
    max_staleness_secs: i64,
    max_confidence_bps: u16,
) -> bool {
    (initial_margin_bps == 500 && maintenance_margin_bps == 250)
        || (initial_margin_bps == 200
            && maintenance_margin_bps == 100
            && max_staleness_secs > 0
            && max_staleness_secs <= 2
            && max_confidence_bps > 0
            && max_confidence_bps <= 100)
}