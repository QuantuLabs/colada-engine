# Percolator engine, as modified by Colada

*Derived from aeyakovenko/percolator at d8dad98 (Apache-2.0); modified by Colada contributors.*

This directory is the [Percolator](https://github.com/aeyakovenko/percolator)
slab accounting engine, forked at `d8dad9893f384f122769b0c5bc6cd17fbc426edc`
and modified by Colada. The crate is named `colada`. It contains the engine,
unit tests, regressions, an optional fuzz suite and 147 Kani harnesses, and has
no runtime dependencies. The changes from upstream are listed in
[Provenance](../docs/PROVENANCE.md).

Colada's on-chain program does not link this crate. It uses a base-position
accounting model that differs from the `colada-engine` crate at the repository
root, and its proofs do not cover that crate.

- [`spec.md`](spec.md): the engine specification.
- [`audit.md`](audit.md): the upstream proof timing and audit notes, kept as an archive.
- `kani_audit_full.tsv`: the upstream result table, unchanged.

Current outcomes are in the root [result matrix](../verification/kani-results.tsv);
domains, conditional assertions and unresolved cases are in the
[verification guide](../docs/VERIFICATION.md).

Run from the repository root:

```sh
cargo test --locked -p colada --features test
cargo test --locked -p colada --features test,fuzz
# Linux: one harness, or all harnesses with KANI_CONTINUE_ON_FAILURE=1
KANI_PACKAGE=colada bash scripts/run_kani_full_audit.sh <harness_name>
```

Kani runs with four accounts; host tests use 64. The harness
`proof_NEGATIVE_bypass_set_pnl_breaks_invariant` is an expected-panic control,
not a safety proof.
