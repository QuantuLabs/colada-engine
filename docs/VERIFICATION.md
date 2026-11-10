# Verification

## Rust tests

```bash
cargo check --workspace --locked
cargo test --workspace --features colada/test,colada/fuzz --locked
```

Tests: 26 in `colada-engine` and 133 in `colada`. One extended fuzz test is ignored by default; run it
with `--ignored`. The root crate's tests replay the conformance vectors in
`tests/fixtures/` and check arithmetic boundaries. Default workspace commands
select only the root crate; `--workspace` adds the Percolator engine (`colada`).

## Kani harnesses

| Package | Source | Harnesses |
| --- | --- | ---: |
| `colada-engine` | `tests/kani*.rs` | 133 |
| `colada` (Percolator engine) | `percolator/tests/kani.rs` | 147 |

The root harnesses are grouped by module. A family repeats one property over
fixed parameter values, one harness per value; the union of its instances is
the whole claim.

`math` (`tests/kani.rs`):

| Harness or family | Instances | Bounded claim |
| --- | ---: | --- |
| `funded_net_respects_caps` | 1 | Size magnitude 1..1000, prices 1..10000, integral Q64 indices −8..8, allocation 0..1000: the net is capped between the negative allocation and the absolute notional. |
| `funded_mixed_price_and_half_index_signed_floor` | 1 | Odd magnitude 1..31, mixed prices, half-unit funding index, both signs and credit modes: exact signed net and cap, and the invalid-envelope branches. |
| `debit_only_fractional_funding_rounds_against_payer` | 1 | All signs, size 1..16, whole funding 0..4, fractional residue 1..1000: a debit rounds against the payer and a credit is dropped when disabled. |
| `q64_floor_ceil_bounded_product` | 1 | `a`, `b` in 1..16 with full `u16` residues: exact Q64 floor and ceil, gap at most 1. |
| `q64_narrowing_boundary_floor_ceil_split` | 1 | `(2^96 − t)(2^96 + t)` for every `u16` `t`: exact floor/ceil split at the `u128` boundary. |
| `u256_add_and_double_carry_overflow` | 1 | Full `u64` low and top limbs, maximal middle limbs, `u8` addend: exact carry and overflow; doubling with a full top limb. |
| `u256_top_limb_division_ceil_and_zero_denominator` | 1 | Numerator `2^192 + k·2^128 + tail`, `k` in 0..7, `u8` tail: ceil division by `2^128`, narrowing refusal, zero denominator. |
| `authenticated_cap_and_u256_narrowing_full_limbs` | 1 | Four full `u64` limbs and a full `u64` cap: exact minimum, and narrowing succeeds iff the upper limbs are zero. |
| `conversion_validation_precedence` | 1 | Full `u64` amount, `u128` bounds, `u8` decimals: decimal, zero-price and envelope errors in their documented order; bound ordering. |
| `composed_dispersion_nondecreasing_*` | 6 | One argument ordered and symbolic in 0..4000, the other fixed at 0, 1 or 4000, on each axis: monotonic, at least the larger input, at most 10,000. Other fixed values and joint moves are not covered. |
| `identity_quote_exact` | 1 | Identity price, six decimals, amount 1..10000: the quote equals the amount. |
| `identity_inverse_exact_*` | 2 | Identity price, six decimals: payout exact for 1..10000; debit exact at 1 and 10000. |
| `collateral_fractional_value_haircut_and_impairment_*` | 16 | Price `k/4` with `k` in {1, 3, 16, 31}, decimals 6..9, atoms in {0, 1, 5, 65535}: exact floored value, half-value credit and impairment ceiling. |
| `collateral_inverse_fractional_rounding_*` | 24 | Price `k/4` with `k` in {1, 3, 16}, decimals 6..9: the payout for quotes 0..1000 is the largest amount not exceeding the quote; the debit for quotes {0, 1, 3, 1000} is the smallest amount covering it. |
| `open_allocation_availability_and_narrowing_precedence` | 1 | Margin 0..1000 or `2^63`, full `u64` free atoms, full `u8` decimals: error precedence and exact outputs. |
| `open_allocation_availability_and_narrowing_precedence_*` | 5 | The same property for margin in {0, 1, 1000} at decimals 6, 7, 8 and 9 with full `u64` free atoms, and for invalid decimals 0, 5, 10 and 255. |
| `final_debit_partition_exact_cover_and_wide_clamp_*` | 6 | Loss in {0, 1, 2, 3, 255} × eligible in {0, 1, 2, 255} at price 1.5, six decimals; a wide case at nine decimals with eligible in {0, 1, MAX}: exact debit, coverage and `covered + uncovered = loss`. |
| `secure_reference_frozen_and_zero_elapsed` | 1 | All three modes, bounded prices and clocks: a frozen or zero-elapsed step leaves the reference unchanged. |
| `secure_reference_nonzero_elapsed_rate_and_step_cap_*` | 10 | Base 100 or 65535, fraction 0..255 or 65535, elapsed time limited by the Unix clock (1 or 16 s) or by the slot clock (1 or 16 slots, 0.4 or 6.4 s), and the capped regime: exact rate floor, 1% step cap, snap inside the band, clock and flag preservation. |
| `secure_fill_checked_directional_rounding_bounds_*` | 16 | Both directions, size in {1, 7, 255}, eight fixed price/reference pairs: checked and saturating fills agree; buys round up and sells round down with a minimum of 1. |
| `confidence_rounding_matches_symmetric_breaker_band_*` | 3 | `k` in {1, 2, 32}, deviations {0, 1, k, k+1, 20k−1}, both sides: exact confidence ceiling, the 500 bps breaker on both sides, and the error branches. |

`funding` (`tests/kani_funding.rs`):

| Harness or family | Instances | Bounded claim |
| --- | ---: | --- |
| `funding_slot_advancement` | 1 | Elapsed `u16`, start slot `u32`, rate `i16`: the slot advances by the elapsed count; `changed` iff it is nonzero. |
| `funding_index_zero_elapsed_identity` | 1 | Any `i128` index, carry and rate, any `u64` slot: a zero-slot step returns its inputs. |
| `funding_index_carry_reconstructs_numerator_*` | 4 | Rate and carry `i16`, elapsed 1, 2, 256 or 9000: `index·9000 + remainder = rate·elapsed + carry` and `|remainder| < 9000`. |
| `funding_index_partitioned_accrual_matches_single_step_*` | 4 | Rate `i16`, elapsed pairs (1, 1), (1, 255), (256, 256), (4500, 4500): two consecutive steps equal one step over the sum. |
| `funding_rate_deadband_and_cap` | 1 | Premium `i32` plus the `i64` extrema: the rate is within ±10 bps, has the premium's sign or is zero, and is zero inside the deadband. |
| `funding_long_idle_pnl_bounded` | 1 | Size `i16`, premium `i32`, 65,535 idle slots: `|pnl| <= |size| / 125 + 1`. |
| `funding_pnl_floor_remainder_canonical` | 1 | Size `i8`, index delta `i16·2^56`, any `u64` carry: `pnl·2^64 + remainder` equals the numerator and the PnL is its floor. |
| `price_pnl_one_directional_rounding` | 1 | Size and execution price 1..256, entry 100, both directions: the PnL is the floor of the exact quotient. |

`margin` (`tests/kani_margin.rs`):

| Harness or family | Instances | Bounded claim |
| --- | ---: | --- |
| `margin_requirement_is_ceiling_*` | 2 | Notional `u16`, rate 500 or 200 bps: the requirement is the exact ceiling. |
| `margin_requirement_monotone_notional` | 1 | Ordered `u16` notionals at 500 bps: the requirement does not decrease. |
| `margin_requirement_boundaries` | 1 | Concrete 0, 1 and maximal inputs, including the overflow case. |
| `margin_requested_current_policy` | 1 | All `u16` requested and market rates: accepted iff equal, or `market <= requested <= 5000`. |
| `baseline_margin_pair_preserved` | 1 | Any oracle settings and version: the pair (500, 250) is admissible. |
| `tier1_requires_strict_oracle_bounds_and_template` | 1 | Any oracle settings and version: (200, 100) is admissible iff staleness is 1..2 s and confidence 1..100 bps, plus an enabled oracle and version >= 1 in the six-argument form. |
| `invalid_margin_pairs_fail_closed` | 1 | All `u16` pairs under admissible oracle settings: only (500, 250) and (200, 100) pass. |

`discovery` (`tests/kani_discovery.rs`):

| Harness or family | Instances | Bounded claim |
| --- | ---: | --- |
| `depth_budget_bounded_by_policy_cap` | 1 | Depth `u32` at the full multiple: the budget equals the depth and stays under the cap. |
| `depth_budget_single_row_no_greater_than_multirow` | 1 | Depth `u32`: the single-row budget is `floor(depth / 4)` and never exceeds the multi-row budget. |
| `point_caps_monotone_and_ordered` | 1 | Ordered `u16` floors, both cap modes: caps are monotonic and the position cap never exceeds the market cap; saturation at `u128::MAX`. |
| `sampled_floor_conversion_rounds_down` | 1 | Floor `u16`, price 100, nine decimals: the conversion is the floor of the exact quotient. |
| `robust_cap_tightens_with_deviation_and_safety` | 1 | Positive `u16` floor: doubling the deviation or the safety factor halves the cap; exact closed forms. |
| `live_notional_rejects_above_any_cap` | 1 | Equity 10000..75535, policy 1..65536, 10% vault and 20% closeout terms: the limit is accepted and the limit plus one rejected. |
| `sampled_support_requires_120s_common_overlap` | 1 | Start `u16`, duration `u8`, two legs with one sampled: accepted iff the common interval is at least 120 s. |
| `zero_floor_or_zero_cap_parameters_rejected` | 1 | Positive `u16` floor: each zero parameter is rejected; overflow and unit boundaries. |

`solvency` (`tests/kani_solvency.rs`):

| Harness or family | Instances | Bounded claim |
| --- | ---: | --- |
| `ledger_partition_equals_recognized_atoms` | 1 | Five `u16` balances and concrete edges: free atoms plus the nonfree partitions equal the recognized balance. |
| `vault_donation_does_not_create_free_claims` | 1 | Six `u16` values: vault atoms above the recognized balance do not change free atoms. |
| `ledger_rejects_underbacked_or_overflowed_partition` | 1 | Positive `u16` balances: an under-backed ledger, each overflow stage and a short vault are rejected.|

Every root harness calls the production function; no arithmetic is stubbed.
Every family head carries a `kani::cover!` on its success path.

### Running a harness

Linux only. Install Kani 0.67.0, then run harnesses by exact name:

```bash
cargo install --locked kani-verifier --version 0.67.0
cargo kani setup
bash scripts/run_kani_full_audit.sh funded_net_respects_caps
KANI_PACKAGE=colada bash scripts/run_kani_full_audit.sh fast_i2_deposit_preserves_conservation
# Every harness of a package, recording all outcomes:
KANI_PACKAGE=colada KANI_CONTINUE_ON_FAILURE=1 bash scripts/run_kani_full_audit.sh
```

`python3 scripts/kani_ci.py list <package>` prints the harness names. The runner
rejects unknown names before running anything, then runs each harness with
`--exact --jobs 1 --default-unwind 70` (a harness's own unwind attribute takes
precedence) under a soft and hard virtual-memory limit of at most 8,000,000 KiB
and a GNU `timeout` with a five-second kill grace.

| Variable | Default | Meaning |
| --- | --- | --- |
| `KANI_PACKAGE` | `colada-engine` | `colada-engine` or `colada` |
| `KANI_OBJECT_BITS` | `12` | CBMC object bits: 12, 13 or 14 |
| `KANI_TIMEOUT_SECONDS` | `600` | Per-harness limit, 60–3600 |
| `KANI_CONTINUE_ON_FAILURE` | unset | `1` keeps going after a non-PASS; the exit status still fails |
| `KANI_RESULTS_FILE` | unset | Also write the results TSV to this path |

Logs and `results.tsv` go to a new directory under
`${CARGO_TARGET_DIR:-target}/kani-audit/`. Each outcome is classified as `PASS`
(Kani exit 0), `FAIL` (a failed check or counterexample), `TIMEOUT` or
`INCONCLUSIVE` (resource or tool error without a failed check). Raising object
bits or the timeout changes resources, not the property checked.

## CI

[`verify.yml`](../.github/workflows/verify.yml) runs on every push to `main` and
every pull request, on `ubuntu-24.04` with Rust 1.98.0:

1. The locked check and test commands above.
2. `kani_ci.py plan` splits every harness of both packages into 16 shards,
   balanced by recorded run time.
3. Each shard runs the runner with a 1500-second limit per harness and object
   bits 14 for `colada-engine`, 12 for `colada`.
4. `kani_ci.py gate` prints every harness outcome and fails the run if any
   harness reports `FAIL`, any harness has no result, or any harness recorded
   `PASS` in the matrix does not pass again.

A green run therefore means: no counterexample, and no regression of a recorded
PASS. It does not mean every harness passed; harnesses not recorded as PASS may
end in `TIMEOUT` or `INCONCLUSIVE` and remain listed. The same gate runs
locally: write each package's results to `<dir>/<package>/results.tsv` with
`KANI_RESULTS_FILE`, then run `python3 scripts/kani_ci.py gate <dir>`.

## Result matrix

[`verification/kani-results.tsv`](../verification/kani-results.tsv) has one row
per harness: `package`, `harness`, `kind` (`positive` or `negative_control`),
`status`, `seconds` and `object_bits`. The rows are the outcomes of one CI
matrix run on `ubuntu-24.04` with a 900-second limit. CI allows 1500 seconds so
that a recorded PASS keeps headroom on a slower runner. A harness without a row is
`UNRECORDED`. A changed harness, or a change to the code it calls, needs a new
CI result before its row is updated.

Five harnesses do not conclude in the recorded run. None reports a failed check.

| Package | Harness | Outcome |
| --- | --- | --- |
| `colada-engine` | `open_allocation_availability_and_narrowing_precedence` | Time limit. The `_*` family proves the same property on narrower domains. |
| `colada-engine` | `u256_top_limb_division_ceil_and_zero_denominator` | Time limit. |
| `colada` | `proof_gap3_multi_step_lifecycle_conservation` | Solver out of memory under the 8 GB limit. |
| `colada` | `proof_lifecycle_trade_warmup_withdraw_topup_conservation` | Solver out of memory under the 8 GB limit. |
| `colada` | `proof_lifecycle_trade_crash_settle_loss_conservation` | Time limit in the recorded run. Its run time varies and it concludes on some runners. |

`percolator/kani_audit_full.tsv` is the upstream result table, kept
unchanged. It is not part of the matrix.

## What the proofs establish

- Each harness proves its assertions only over its stated finite input domain,
  not over the whole numeric range.
- Harness counts are not counts of independent guarantees; parameterized
  families repeat one property over several fixed values.
- `proof_NEGATIVE_bypass_set_pnl_breaks_invariant` is a negative control marked
  `#[kani::should_panic]`: it must reach the violation, and its PASS is reported apart from positive
  results.
- The Percolator engine runs Kani with four accounts (host tests use 64) and
  primitive 128-bit integers. It does not prove the full-size account layout.
- Some inherited Percolator harnesses assert only after an operation succeeds,
  and one lifecycle harness assumes crank and close succeed. The single
  funding-crank fixtures install a next-interval rate and do not exercise a
  nonzero elapsed funding payment. `proof_lq3a_profit_routes_through_adl` does not establish profit
  routing through ADL.
- The Percolator-engine harnesses are the set inherited at the fork point. In the
  following week upstream strengthened most of them (symbolic instead of fixed
  inputs, the full invariant checked before and after each operation); 128 of
  the 147 harnesses here keep the earlier, weaker form. See
  [Provenance](PROVENANCE.md).
- Percolator-engine harnesses set their initial clocks and aggregates explicitly; two
  partial-liquidation families use zero fees on purpose.
- Percolator-engine proofs do not cover the root crate, and neither crate's proofs cover
  an integrating program. Review the complete system before mainnet use.
