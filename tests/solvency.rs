use colada_engine::solvency::{LedgerView, SolvencyError};

#[test]
fn recognized_balance_vectors() {
    let value = LedgerView {
        recognized_raw_atoms: 100,
        seed_atoms: 11,
        allocated_user_atoms_total: 19,
        backing_atoms: 13,
        atom_claims_payable_total: 17,
        vault_raw_atoms: 100,
    };
    assert_eq!(value.free_user_atoms_total(), Ok(40));
    assert_eq!(
        LedgerView {
            vault_raw_atoms: 99,
            ..value
        }
        .free_user_atoms_total(),
        Err(SolvencyError::InvalidAccountData)
    );
    assert_eq!(
        LedgerView {
            recognized_raw_atoms: 59,
            ..value
        }
        .free_user_atoms_total(),
        Err(SolvencyError::InvalidAccountData)
    );
    assert_eq!(
        LedgerView {
            seed_atoms: u64::MAX,
            ..value
        }
        .free_user_atoms_total(),
        Err(SolvencyError::ArithmeticOverflow)
    );
}

#[test]
fn partition_vectors() {
    // Recognized, seed, allocated, backing, claims, vault, free.
    for (recognized, seed, allocated, backing, claims, vault, free) in [
        (3_000, 0, 0, 2_000, 0, 3_000, 1_000),
        (110_261, 0, 7_000, 102_600, 661, 110_261, 0),
        (269_511, 0, 7_000, 230_000, 32_511, 269_511, 0),
        (800, 0, 0, 800, 0, 800, 0),
        (12, 0, 0, 12, 0, 12, 0),
        (80, 0, 0, 70, 10, 80, 0),
    ] {
        let ledger = LedgerView {
            recognized_raw_atoms: recognized,
            seed_atoms: seed,
            allocated_user_atoms_total: allocated,
            backing_atoms: backing,
            atom_claims_payable_total: claims,
            vault_raw_atoms: vault,
        };
        assert_eq!(ledger.free_user_atoms_total(), Ok(free));
    }
}
