# Colada Engine

[![verify](https://github.com/QuantuLabs/colada-engine/actions/workflows/verify.yml/badge.svg)](https://github.com/QuantuLabs/colada-engine/actions/workflows/verify.yml)

Risk-engine code for Colada, a perpetual-futures protocol on Solana. It is
built on [Percolator](https://github.com/aeyakovenko/percolator) and keeps its
history.

## Contents

- [`percolator/`](percolator/README.md): the Percolator engine with Colada's
  changes, its tests and its Kani harnesses.
- `src/`: the `colada-engine` crate, the arithmetic of Colada's on-chain
  program as a stateless `no_std` library with no dependencies and
  `forbid(unsafe_code)`. It builds and runs on its own:
  - `math`: 256-bit and Q64.64 arithmetic, collateral conversion with directed
    rounding, funded PnL with caps, reference-price step, fill bounds.
  - `funding`: funding rate, index accrual, funding and price PnL.
  - `margin`: margin requirements and admissible margin pairs.
  - `discovery`: exposure caps derived from liquidity depth.
  - `solvency`: the free-collateral ledger identity.
- `verification/`: the outcome of every Kani harness, enforced by CI.

The two crates are separate and use different accounting models. Colada's
program runs the formulas in `src/` and does not link `percolator/`. Proofs of
one crate do not cover the other.

## What Colada adds

Changes to the Percolator engine (`percolator/`):

- Funding: an absolute rate ceiling, validated rates, and accrual of the old
  rate before a new one is stored, so a rate change never applies to past slots.
- Clocks: every state-changing path rejects a slot earlier than the last one.
- Warmup: profit vests linearly over the whole period, and a trade that adds
  profit restarts it.
- Liquidation: the fee is charged before the margin recheck, and sizing uses
  checked arithmetic with a full-close fallback.
- Execution: a maximum deviation between the matcher price and the oracle price.
- Aggregates: checked open-interest and LP totals; the conservation check also
  recomputes the stored totals.
- Ten fixes ported from later upstream commits, and a regression suite for all
  of these changes. [Provenance](docs/PROVENANCE.md) lists what upstream changed
  since and what is not ported.

New code for Colada's program (`src/`):

- Collateral in tokens of 6 to 9 decimals, valued against a price interval,
  with rounding always against the paying side: debits round up, payouts and
  valuations round down.
- Price and funding PnL in one signed result, capped by the position's
  allocation and notional.
- A reference price that moves at most 2% per hour and 1% per step, fills
  bounded by it, and a 5% deviation breaker.
- Funding index accrual with a carried remainder, so no funding is lost to
  truncation between steps.
- Exposure caps computed from liquidity depth.
- Kani harnesses for these functions, and a CI job that replays the harnesses of
  a crate whenever its code changes and fails on any counterexample or regression.

[Provenance](docs/PROVENANCE.md) has the full list of changes.

## Verify it yourself

```bash
cargo check --workspace --locked
cargo test --workspace --features colada/test,colada/fuzz --locked
# Linux, Kani 0.67.0: one harness, bounded memory and time
bash scripts/run_kani_full_audit.sh funded_net_respects_caps
```

| Crate | Tests | Kani harnesses | Recorded PASS |
| --- | ---: | ---: | ---: |
| `colada-engine` (`src/`) | 26 | 133 | 131 |
| `colada` (`percolator/`) | 144 | 147 | 143 |

Each harness proves its assertions over a stated finite input domain.
[Verification](docs/VERIFICATION.md) lists every domain, the CI matrix and the
harnesses that do not conclude.

## Scope and status

This repository is a library: it computes and holds no state. A Solana program
built on it supplies the accounts, the instruction handlers, the oracle and
the token transfers, and validates every account, signer, price and cap before
calling in. Colada's program runs on Solana devnet and is being moved from its
own copy of these formulas to this crate. Experimental; not claimed ready for
mainnet.

## Use it

```bash
cargo run --example position
```

[`examples/position.rs`](examples/position.rs) walks one position from open to
close: margin, collateral to lock, funding, capped PnL. To depend on the crate:

```toml
[dependencies]
colada-engine = { git = "https://github.com/QuantuLabs/colada-engine.git", rev = "<full-commit-sha>" }
```

The library computes; your program authenticates its inputs and owns the
state. See [Integration](docs/INTEGRATION.md).

## Documentation

- [Specification](spec.md): units, rounding and bounds of `src/`.
- [Integration](docs/INTEGRATION.md): caller boundary and updates.
- [Verification](docs/VERIFICATION.md): tests, proofs, CI and limits.
- [Provenance](docs/PROVENANCE.md): upstream, changes and history.
- [Contributing](CONTRIBUTING.md), [Security](SECURITY.md).

## Licence

[Apache-2.0](LICENSE). Copyright 2026 QuantuLabs; see [NOTICE](NOTICE). Forked from [aeyakovenko/percolator](https://github.com/aeyakovenko/percolator)
at `d8dad9893f384f122769b0c5bc6cd17fbc426edc`; modified by QuantuLabs.
