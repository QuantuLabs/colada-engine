# Contributing

The root crate is a stateless numerical library. Program state, custody and
input authentication belong to integrators and are out of scope.

## Changes

1. Reproduce the behavior with a test, then make the smallest fix where the
   logic lives. Add a regression for each reproduced defect.
2. Keep [spec.md](spec.md), the conformance vectors in `tests/fixtures/` and the
   Kani harnesses in `tests/kani*.rs` in step with the code. A harness's input
   domain and the spec's stated bounds must describe the same thing.
3. Run:

   ```bash
   cargo check --workspace --locked
   cargo test --workspace --features colada/test,colada/fuzz --locked
   ```

4. Run the Kani harnesses that cover the changed code (see
   [Verification](docs/VERIFICATION.md)); CI replays all of them.
5. Describe the behavior change and the checks you ran in the commit message.
   Preserve upstream authorship and commit dates.

## Proofs

- Root-crate harnesses go in `tests/kani.rs` (`math`) and `tests/kani_<module>.rs`;
  Percolator-engine harnesses in
  `percolator/tests/kani.rs`.
- Keep assertions meaningful. Do not weaken an assertion, assume the result,
  or disable unwinding checks to make a harness pass.
- A timeout or resource limit is inconclusive, not a pass. Record it as such.
- A changed harness, or a change to the code it calls, needs a new result.
  Update its row in `verification/kani-results.tsv` in the same change, and add
  a row for each new harness.
- Keep the negative control and report it apart from positive results.

## Files derived from Percolator

Files modified from upstream carry a change notice at the top. Keep it, and add
one when you modify another upstream file.
