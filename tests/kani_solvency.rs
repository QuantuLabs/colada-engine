#![cfg(kani)]

use colada_engine::solvency::{LedgerView, SolvencyError};

// Domain: five independent u16 balances widened to u64; concrete zero/one/MAX edges.
#[kani::proof]
#[kani::unwind(4)]
fn ledger_partition_equals_recognized_atoms() {
    let seed = u64::from(kani::any::<u16>());
    let allocated = u64::from(kani::any::<u16>());
    let backing = u64::from(kani::any::<u16>());
    let claims = u64::from(kani::any::<u16>());
    let free = u64::from(kani::any::<u16>());
    let recognized = seed + allocated + backing + claims + free;
    let ledger = LedgerView {
        recognized_raw_atoms: recognized,
        seed_atoms: seed,
        allocated_user_atoms_total: allocated,
        backing_atoms: backing,
        atom_claims_payable_total: claims,
        vault_raw_atoms: recognized,
    };
    let result = ledger.free_user_atoms_total();
    assert_eq!(result, Ok(free));
    assert_eq!(
        result.unwrap() + seed + allocated + backing + claims,
        recognized
    );
    kani::cover!(result == Ok(free) && free > 0);
    for amount in [0, 1, u64::MAX] {
        let empty = LedgerView {
            recognized_raw_atoms: amount,
            seed_atoms: 0,
            allocated_user_atoms_total: 0,
            backing_atoms: 0,
            atom_claims_payable_total: 0,
            vault_raw_atoms: amount,
        };
        assert_eq!(empty.free_user_atoms_total(), Ok(amount));
        assert_eq!(
            LedgerView {
                seed_atoms: amount,
                ..empty
            }
            .free_user_atoms_total(),
            Ok(0)
        );
    }
}

// Domain: six independent u16 values widened to u64, including donated vault atoms.
#[kani::proof]
#[kani::unwind(2)]
fn vault_donation_does_not_create_free_claims() {
    let seed = u64::from(kani::any::<u16>());
    let allocated = u64::from(kani::any::<u16>());
    let backing = u64::from(kani::any::<u16>());
    let claims = u64::from(kani::any::<u16>());
    let free = u64::from(kani::any::<u16>());
    let donation = u64::from(kani::any::<u16>());
    let recognized = seed + allocated + backing + claims + free;
    let ledger = LedgerView {
        recognized_raw_atoms: recognized,
        seed_atoms: seed,
        allocated_user_atoms_total: allocated,
        backing_atoms: backing,
        atom_claims_payable_total: claims,
        vault_raw_atoms: recognized,
    };
    let before = ledger.free_user_atoms_total();
    let after = LedgerView {
        vault_raw_atoms: recognized + donation,
        ..ledger
    }
    .free_user_atoms_total();
    assert_eq!(before, Ok(free));
    assert_eq!(after, before);
    assert_eq!(
        LedgerView {
            vault_raw_atoms: u64::MAX,
            ..ledger
        }
        .free_user_atoms_total(),
        before
    );
    kani::cover!(after == Ok(free) && donation > 0 && free > 0);
}

// Domain: positive u16 balances, three concrete overflow stages, concrete vault precedence.
#[kani::proof]
#[kani::unwind(4)]
fn ledger_rejects_underbacked_or_overflowed_partition() {
    let amount = u64::from(kani::any::<u16>());
    kani::assume(amount > 0);
    let ledger = LedgerView {
        recognized_raw_atoms: amount,
        seed_atoms: 0,
        allocated_user_atoms_total: 0,
        backing_atoms: 0,
        atom_claims_payable_total: 0,
        vault_raw_atoms: amount,
    };
    assert_eq!(ledger.free_user_atoms_total(), Ok(amount));
    kani::cover!(ledger.free_user_atoms_total() == Ok(amount));
    let underbacked = LedgerView {
        vault_raw_atoms: amount - 1,
        ..ledger
    };
    assert_eq!(
        underbacked.free_user_atoms_total(),
        Err(SolvencyError::InvalidAccountData)
    );
    let excessive = LedgerView {
        seed_atoms: amount + 1,
        ..ledger
    };
    assert_eq!(
        excessive.free_user_atoms_total(),
        Err(SolvencyError::InvalidAccountData)
    );
    for (allocated, backing, claims) in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
        let overflow = LedgerView {
            recognized_raw_atoms: u64::MAX,
            seed_atoms: u64::MAX,
            allocated_user_atoms_total: allocated,
            backing_atoms: backing,
            atom_claims_payable_total: claims,
            vault_raw_atoms: u64::MAX,
        };
        assert_eq!(
            overflow.free_user_atoms_total(),
            Err(SolvencyError::ArithmeticOverflow)
        );
        assert_eq!(
            LedgerView {
                vault_raw_atoms: u64::MAX - 1,
                ..overflow
            }
            .free_user_atoms_total(),
            Err(SolvencyError::InvalidAccountData)
        );
    }
    kani::cover!(underbacked.free_user_atoms_total() == Err(SolvencyError::InvalidAccountData));
}
