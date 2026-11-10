# Integration

## Boundary

The library computes; the calling program decides. The caller
authenticates accounts, owners, signers, prices, mints, decimals and caps before
building library inputs, and it owns state, token transfers, commits and
rollback. Type names such as `AuthenticatedQuoteCap` record what the caller must
have checked; the library does not check it.

The kernel uses signed quote notional and Q64.64 funding. The Percolator engine in
`percolator/` uses a different base-position slab model and is not a
substitute for program accounts or instructions. Do not depend on it from a
program.

## Dependency

```toml
[dependencies]
colada-engine = { git = "https://github.com/QuantuLabs/colada-engine.git", rev = "<full-commit-sha>" }
```

Pin a full commit you have reviewed, not a branch, and commit the lockfile.
A path dependency is convenient for local work on both repositories.

## Errors

Each module has its own error type: `MathError`, `FundingError`, `MarginError`,
`DiscoveryError` and `SolvencyError`. Map each variant to an
explicit program error; never `unwrap` in a program or treat an error as zero.
Keep the mapping stable so that clients can rely on error codes. The
saturating `secure_fill` exists for callers that want saturation; use
`checked_secure_fill` when overflow must abort the transaction.

## Updating the pinned commit

1. Read the diff of `src/` and [spec.md](../spec.md). A change of unit, rounding
   direction, constant, envelope, error variant or funding sign changes the
   economic contract.
2. Run `cargo test --workspace --locked` and the Kani harnesses of the changed
   functions (see [Verification](VERIFICATION.md)), or check that CI passed for
   that commit.
3. Run your own program tests, including error mapping and the open, close and
   liquidation paths, before deploying.
