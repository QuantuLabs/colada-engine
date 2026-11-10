# Provenance

## Upstream

This repository is a fork of [aeyakovenko/percolator](https://github.com/aeyakovenko/percolator)
(Apache-2.0) by Anatoly Yakovenko and Percolator contributors. Its history keeps
the upstream commits from `60219cd0fe8a4f7cb0980105f7c97c26828b0a5f`
(2025-10-19) through the fork point `d8dad9893f384f122769b0c5bc6cd17fbc426edc`
(2026-02-12), with their original authors and dates.

## Colada history

Colada started on Percolator in February 2026. Most of that work happened in
the repository of Colada's on-chain program. The commits
after the fork point are:

- `chore: rename crate entrypoint to colada` (2026-05-24) and
  `test: heap-allocate large risk engine fixtures` (2026-06-01), made on the
  fork itself.
- 38 commits dated 2026-04-09 through 2026-11-04, taken from the program
  repository and filtered to the code published here. Each keeps its original
  author, dates and message. Its content is the source of the functions now in
  `src/` as they were at that commit, stored under `research/kernel-history/`:
  the whole `math` file with its host test and vectors from 2026-08-20, and the
  funding, margin and exposure-cap functions from April. These files are
  excerpts and are not built. A message describes the whole original commit,
  so it can mention code outside this library. Two messages are
  shortened to their first sentence. The commits are in their original order.
- The commits of 2026-11-10, which turn those excerpts into the `colada-engine`
  crate and move the Percolator engine to `percolator/`. Colada's changes to
  the Percolator engine, listed below, were developed in a working tree and
  are first committed here.

`solvency` has no earlier history in this repository: before the extraction it
was a method of a program account type.

## `colada-engine` crate

The modules in `src/` are extracted from the source of Colada's on-chain
program as of 2026-11-05. `math` was a single file of the program; `funding`,
`margin`, `discovery` and `solvency` collect pure functions from larger files.
Function bodies are unchanged. The extraction gives them neutral names,
plain-value parameters and one error type per module in place of the program
error. The program is being moved from its own copy of these functions to this
crate. The contract is defined by the source, [spec.md](../spec.md) and the
conformance vectors in `tests/fixtures/`.

## Percolator engine

`percolator/` is a modified derivative of upstream `src/percolator.rs`,
`src/i128.rs`, the upstream tests, `spec.md` and `audit.md`. The crate and its
entry file are renamed `colada`. Runtime changes from `d8dad98`:

- Funding: an absolute ceiling of 100 bps per slot and a configurable maximum;
  stored and accrued rates are validated; `accrue_funding_with_rate` accrues the
  old rate before storing the new one (anti-retroactive).
- Clocks: deposit, withdraw, trade, crank, liquidation, close and fee-credit
  paths reject a slot earlier than the last one; funding accrual rejects a
  backwards slot.
- Warmup: zero warmup is sanitized to one slot; vesting uses
  `floor(available / period)` with full release after one period instead of a
  minimum slope of one; a trade that increases available PnL restarts warmup.
- Liquidation: the fee is charged before the residual margin check, only the
  incremental capped fee is taken from capital, sizing uses checked
  multiplication with a full-close fallback, and the crank grants no fee
  forgiveness and force-closes dust only below maintenance.
- Execution: a configurable maximum deviation of the matcher price from the
  oracle price.
- Aggregates: open-interest and LP aggregates use checked arithmetic and return
  `Overflow`; `check_conservation` also recomputes `c_tot` and `pnl_pos_tot` and
  checks account kinds and reserved PnL.
- Interfaces: initialization returns `Result` and rejects a live state;
  parameters are sanitized (account cap, freshness below warmup, deviation);
  `AccountKind` is stored as a raw byte; `set_owner` checks a caller-supplied
  owner; `set_reserved_pnl` bounds reserves by positive PnL.
- `i128.rs`: `abs` and negation saturate at `MIN`.

Tests were adapted to these interfaces and a regression suite was added
(`tests/audit_regressions.rs`). Its harnesses received fixture corrections
(monotonic clocks, consistent aggregates, explicit zero-fee cases) and tighter
loop or type bounds, with unwinding assertions kept. Each modified file carries
a change notice. `tests/fuzzing.proptest-regressions` and `kani_audit_full.tsv`
are unchanged.

### Upstream after the fork point

Upstream kept changing after `d8dad98`. It strengthened its Kani harnesses from
2026-02-13 to 2026-02-19, rewrote the engine from 2026-03-13 and removed
`src/percolator.rs` on 2026-05-14. That history is not merged here;
`percolator/` stays on the February architecture.

Fixes ported from upstream, each with a regression test in
`tests/audit_regressions.rs`:

| Change | Upstream commits |
| --- | --- |
| Liquidation restarts warmup when mark settlement adds available profit | `700b8564` |
| Constructors reject a maintenance margin above the initial margin | `da085a00`, `39ae652c` |
| `deposit_fee_credits` rejects an amount that overflows the signed credit balance | `c3933293` |
| Garbage collection keeps an account that holds prepaid fee credits | `4777cf50`, `f7d2de85` |
| Fee debt is swept when warmup converts profit to capital | `1ab35740` |
| Account creation, deposits, fee-credit deposits and insurance top-ups reject an inflow that overflows the vault, insurance or capital totals | `1ab35740`, `b65a14a1`, `f4930c2e` |
| Equity is evaluated exactly instead of clamping capital before adding PnL | `640055a8` |
| Funding accrual fast-forwards an idle period when the stored rate is zero | `379c3cac`, `072a62eb`, `af18b090` |
| Constructors reject fee rates above 10,000 bps and the trading fee uses checked arithmetic | current upstream behaviour |
| `set_owner` rejects the all-zero owner | `8685828a` |

Known differences that are not ported:

- Haircut products (`effective_pos_pnl`, profit conversion) multiply with
  saturation before dividing, so backed profit is undercounted when
  `pnl × haircut numerator` exceeds `2^128`. Upstream moved to wide arithmetic
  in `9bc0fab5`.
- After the slot advances by more than 31,536,000 without funding accrual at a
  nonzero rate, the crank returns `Overflow` until accrual catches up in bounded
  steps.
- Internal setters and aggregate recomputation still use saturating additions;
  only the public inflow paths above are checked.
- Funding is quantized per accrual call, as in the February specification, so
  many short accruals can yield less funding than one long accrual. Upstream
  later added a remainder.
- The keeper crank ignores errors from per-account maintenance-fee settlement,
  funding settlement and forced close, so an account it cannot settle is skipped
  instead of failing the crank.
- The February harness strengthening (upstream `301ab914` to `e0956618`): 128 of
  the 147 harnesses keep the earlier form.

The two crates use different accounting models. Proofs of the Percolator engine do not cover
the root crate; see [Verification](VERIFICATION.md).

## Licence

The upstream [Apache-2.0 licence](../LICENSE) is retained unchanged. The root crate and
the changes to the Percolator-derived files are Copyright 2026 QuantuLabs; the
[NOTICE](../NOTICE) file records both.
