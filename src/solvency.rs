//! Recognized balance-sheet partitions and free user atoms.

/// Errors in the supplied balance-sheet arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SolvencyError {
    /// The vault or recognized balance cannot cover the supplied partitions.
    InvalidAccountData,
    /// Adding the nonfree partitions exceeded the integer range.
    ArithmeticOverflow,
}

/// Caller-supplied balances in atoms; vault donations are not recognized funds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerView {
    pub recognized_raw_atoms: u64,
    pub seed_atoms: u64,
    pub allocated_user_atoms_total: u64,
    pub backing_atoms: u64,
    pub atom_claims_payable_total: u64,
    pub vault_raw_atoms: u64,
}

impl LedgerView {
    /// Caller must supply authentic, mutually consistent balances and validate their context.
    pub fn free_user_atoms_total(&self) -> Result<u64, SolvencyError> {
        if self.vault_raw_atoms < self.recognized_raw_atoms {
            return Err(SolvencyError::InvalidAccountData);
        }
        let recognized_nonfree_atoms = self
            .seed_atoms
            .checked_add(self.allocated_user_atoms_total)
            .and_then(|value| value.checked_add(self.backing_atoms))
            .and_then(|value| value.checked_add(self.atom_claims_payable_total))
            .ok_or(SolvencyError::ArithmeticOverflow)?;
        self.recognized_raw_atoms
            .checked_sub(recognized_nonfree_atoms)
            .ok_or(SolvencyError::InvalidAccountData)
    }
}
