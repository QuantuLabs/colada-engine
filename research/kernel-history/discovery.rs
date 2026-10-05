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

/// Position and shared FeedOI caps from an authenticated USDC6 depth floor.
pub fn sampled_caps(floor_usdc:u128)->(u128,u128) {
    let cap=|ratio:u128,absolute:u128| floor_usdc.checked_mul(ratio).map_or(absolute,|v|(v/10_000).min(absolute));
    (cap(1_000,SAMPLED_POSITION_ABSOLUTE),cap(2_500,SAMPLED_SHARED_ABSOLUTE))
}

/// floor(F * lowerQ64 * 1e6 / (10^decimals * 2^64)); USDC6 passes F unchanged.
pub fn sampled_floor_usdc(floor:u128,lower_x64:u128,quote_decimals:u8)->Result<u128,ProgramError> {
    if quote_decimals>18 || lower_x64==0 {return Err(bad());}
    let mut wide=[0u64;5];
    for (i,a) in [floor as u64,(floor>>64) as u64].into_iter().enumerate() {
        let mut carry=0u128;
        for (j,b) in [lower_x64 as u64,(lower_x64>>64) as u64].into_iter().enumerate() {
            let v=u128::from(a)*u128::from(b)+u128::from(wide[i+j])+carry;wide[i+j]=v as u64;carry=v>>64;
        }
        wide[i+2]=carry as u64;
    }
    let mut carry=0u128;
    for limb in &mut wide {let v=u128::from(*limb)*1_000_000+carry;*limb=v as u64;carry=v>>64;}
    if carry!=0 || wide[4]!=0 {return Err(bad());}
    let divisor=10u128.pow(u32::from(quote_decimals));let mut rem=0u128;
    for limb in wide.iter_mut().rev() {
        let v=(rem<<64)|u128::from(*limb);*limb=(v/divisor) as u64;rem=v%divisor;
    }
    if wide[3]!=0 || wide[4]!=0 {return Err(bad());}
    Ok(u128::from(wide[1])|(u128::from(wide[2])<<64))
}

pub type Support=Option<(u32,u32)>;

pub fn meet(a:Support,b:Support)->Support {
    match (a,b) {(Some((s0,e0)),Some((s1,e1)))=>Some((s0.max(s1),e0.min(e1))),(a,None)=>a,(None,b)=>b}
}

/// Common support of priced legs; with any sampled leg, every leg must be supported and all must share >=120 s.
#[inline(never)]
pub fn overlap(legs:&[(Support,bool)])->Result<Support,ProgramError> {
    let real=||legs.iter().filter(|(support,_)|*support!=POINT);
    let n=real().count();
    if n==0 && !legs.is_empty() {return Ok(POINT);}
    let sampled=(n>1 || n<legs.len()) && real().any(|(_,sampled)|*sampled);
    if real().any(|(support,_)|support.is_none()) {
        if sampled {return Err(bad());}
        return Ok(None);
    }
    let common=real().fold(None,|acc,(support,_)|meet(acc,*support));
    if sampled && common.is_none_or(|(start,end)|end.checked_sub(start).is_none_or(|span|span<120)) {return Err(bad());}
    Ok(common)
}

pub fn span(start:i64,end:i64)->Result<(u32,u32),ProgramError> {
    Ok((u32::try_from(start).map_err(|_|bad())?,u32::try_from(end).map_err(|_|bad())?))
}

/// Type tag of a current-point leg: it earns no interval, `overlap` excludes it and any `meet` with it is empty.
pub const POINT:Support=Some((u32::MAX,u32::MAX));

pub fn secure_caps(floor_usdc:u128)->(u128,u128) {
    let cap=|ratio:u128,absolute:u128| floor_usdc.checked_mul(ratio).map_or(absolute,|v|(v/10_000).min(absolute));
    (cap(250_000,SAMPLED_POSITION_ABSOLUTE),cap(350_000,SAMPLED_SHARED_ABSOLUTE))
}

pub fn point_caps(floor_usdc:u128,secure_lift:bool)->(u128,u128) {
    if secure_lift {secure_caps(floor_usdc)} else {sampled_caps(floor_usdc)}
}

#[inline(always)]
pub fn bounded_worst_case_closeout_notional_for_liquidity(
    max_trader_open_interest_bps: u16,
    liquidity_basis: u64,
) -> Result<u64, ProgramError> {
    let cap = u128::from(liquidity_basis)
        .checked_mul(u128::from(max_trader_open_interest_bps))
        .ok_or(ProgramError::from(ColadaError::Overflow))?
        / 10_000;
    if cap == 0 {
        return Err(ProgramError::from(ColadaError::InvalidConfig));
    }
    u64::try_from(cap).map_err(|_| ProgramError::from(ColadaError::Overflow))
}

#[inline(always)]
pub fn validate_cap_v2_live_post_notional(
    max_trader_open_interest_bps: u16,
    free_equity: u64,
    kappa_vault_bps: u16,
    max_notional_policy: u128,
    post_abs_notional: u64,
) -> ProgramResult {
    let r_vault = u128::from(free_equity)
        .checked_mul(u128::from(kappa_vault_bps))
        .ok_or(ProgramError::from(ColadaError::Overflow))?
        / 10_000;
    let closeout_cap = u128::from(bounded_worst_case_closeout_notional_for_liquidity(
        max_trader_open_interest_bps,
        free_equity,
    )?);
    let r_vault_live = r_vault.min(closeout_cap);
    let post_cap = u128::from(MAX_POSITION_NOTIONAL_QUOTE_E6)
        .min(max_notional_policy)
        .min(r_vault_live);
    if post_cap == 0 || u128::from(post_abs_notional) > post_cap {
        return Err(ColadaError::ExecutionBoundsExceeded.into());
    }
    Ok(())
}