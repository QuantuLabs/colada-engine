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