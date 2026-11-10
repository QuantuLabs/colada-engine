# Numerical contract

This is the contract of the `colada_engine` modules: `math` (wide and
fixed-point arithmetic, collateral conversion, funded PnL, reference price and
fills), `funding` (funding rate, index and carry), `margin` (requirements and
admissible margin pairs), `discovery` (depth-based exposure caps) and
`solvency` (free-atom ledger identity). The source and the conformance vectors
in `tests/fixtures/` define exact integer behavior; this document states units,
rounding and bounds. Sections without a module name describe `math`.

## Units

| Quantity | Type | Representation |
| --- | --- | --- |
| Quote amount | `QuoteE6 = u64` | Quote units × 10^6 |
| Wide quote / atoms intermediate | `U256` | Four little-endian `u64` limbs |
| Collateral amount | `CollateralAtoms = u64` | Raw token atoms; decimals `d` in 6..=9 |
| Collateral price | `PriceQ64 = u128` | Quote per whole token, Q64.64 |
| Price interval | `PriceBoundsQ64` | `0 < p_lo <= p_hi`, built by `checked_price_bounds` |
| Funding checkpoint/index | `i128` | Signed Q64.64 |
| Position size | `i64` | Signed quote notional |
| Reference price | `u128` | Q32.32 (`SECURE_ONE_Q32 = 2^32`) |
| Confidence / dispersion | `u16` | Basis points |

Errors are `MathError` variants. Invalid `d` returns `InvalidDecimals`, a zero
price returns `ZeroPrice`, and a `U256` result that does not fit `u64` returns
`InvalidNarrowing`.

## Arithmetic

- `mul_q64_floor(a, b)` = `floor(a·b / 2^64)`; `mul_q64_ceil` rounds up when the
  low 64 bits of the product are nonzero. Both reject a zero operand and a result
  above `u128::MAX`.
- `U256::div_ceil_bounded` rounds up and rejects a zero denominator;
  `checked_add` and `checked_mul_u64` reject overflow.
- `compose_dispersion_bps(a, b)` = `a + b + ceil(a·b / 10_000)`; results above
  10_000 return `OutOfEnvelope`.
- `compose_confidence_bps(point, lo, hi)` = `ceil(max(point−lo, hi−point)·10_000 / point)`.
  It requires `0 < lo <= point <= hi` and a result that fits `u16`.

## Collateral

With `s = 10^d`:

| Function | Result | Rounding |
| --- | --- | --- |
| `quote_e6_floor_wide(atoms, p, d)` | `atoms·p·10^6 / (s·2^64)` | down |
| `atoms_debit_ceil_wide(q, p, d)` | `q·s·2^64 / (p·10^6)` | up |
| `atoms_payout_floor_wide(q, p, d)` | `q·s·2^64 / (p·10^6)` | down |
| `credited_quote_e6_wide(atoms, p_lo, d)` | `quote_e6_floor_wide(atoms, p_lo, d) · 5000 / 10_000` | down |
| `displayed_subatom_impairment_bound_quote_e6(bounds, d)` | `p_hi·10^6 / (s·2^64)` | up |

Debits round against the payer and payouts against the receiver. Overflow of
the atom-conversion numerator returns `OutOfEnvelope`.
`cap_quote_wide(v, cap)` returns `min(v, cap)` as `u64`; `AuthenticatedQuoteCap`
is a plain wrapper and does not authenticate its value.

`open_pre_haircut_requirement(im, bounds, d, free)` computes the pre-haircut
quote `ceil(im·10_000 / 5000)`, then its atom debit at `p_lo`. Errors take this
order: decimals, conversion overflow, narrowing to `u64`, then `InsufficientFreeAtoms` when the
debit exceeds `free`.

`final_debit_partition(loss, bounds, d, eligible)` computes the theoretical
debit of `loss` at `p_lo`. If it fits within `eligible`, the whole loss is
covered. Otherwise `eligible` atoms are debited, the covered loss is
`min(quote_e6_floor_wide(eligible, p_lo, d), loss)`, and
`covered + uncovered = loss`. The theoretical debit stays wide and is not narrowed.

`identity_price_bounds_by_address_equality` returns `[2^64, 2^64]` only for two
equal, nonzero 32-byte mints with 6 decimals.

## Funded PnL

`funded_net_and_capped(size, entry, exit, checkpoint, current, allocation, credit)`
requires `0 < |size| <= 2^44` and nonzero `entry` and `exit`; otherwise it returns
`OutOfEnvelope`.

- Price term: `size·(exit − entry) / entry`, floored toward negative infinity.
- Funding term: `|size|·|current − checkpoint| / 2^64`, floored toward negative
  infinity; it is negative when a long sees the index rise or a short sees it fall.
- With `credit = false` a positive funding term is dropped; debits remain.
- Returns `(raw, capped)` with `raw = price + funding` and
  `capped = clamp(raw, −allocation, |size|)`.

The caller supplies the allocation in quote-e6 units and owns any funding
state, including carried remainders.

## Reference price and execution

Constants: `SECURE_RATE_BPS_PER_HOUR = 200`, `SECURE_STEP_CAP_BPS = 100`,
`SECURE_SLOT_MS = 400`, `SECURE_BREAKER_BPS = 500`, `SECURE_BREAKER_BOTH_SIDES = true`,
`SECURE_IM_FLOOR_BPS = 5000`, `SECURE_P_MAX_Q32 = u64::MAX·2^32`.

`secure_reference_step(r, p, slot, unix, mode)`:

- `Frozen` returns `r` unchanged.
- Elapsed time is `min(max(unix − r.unix, 0)·1000, (slot − r.slot)·400)` ms,
  with slot underflow saturating to zero.
- The band half-width is `floor(r.ref_q32 · min(200·elapsed_ms, 100·3_600_000) / (10_000·3_600_000))`:
  at most 2% per hour and at most 1% per step. The band is
  `[ref − delta, min(ref + delta, SECURE_P_MAX_Q32)]`.
- A price `p·2^32` inside the band replaces the reference. Outside it, `Fresh`
  with `delta = 0` returns `r` unchanged; otherwise the reference is clamped to
  the band.
- Updated states keep `lifted` and set `slot` and `unix` to the maximum of old and new.

Callers must keep `ref_q32 <= SECURE_P_MAX_Q32`; the helpers do not validate it.

`checked_secure_fill(x, p, ref_q32, buy)` uses `s = max(p·2^32, ref)` for buys and
`min(p·2^32, ref)` for sells. Buys return `ceil(max(ceil(s·x/p), s) / 2^32)`;
sells return `max(floor(min(floor(s·x/p), s) / 2^32), 1)`. The division
uses `max(p, 1)` as its divisor; `p·2^32` keeps the original `p`. Overflow and
narrowing return an error. `secure_fill` computes the same bounds but saturates
on overflow: buys return `u64::MAX`, sells return 1.

`secure_breaker_allows_open(p, ref_q32, buy)` is true iff
`|p·2^32 − ref|·10_000 <= 500·ref`, on both sides.

## Funding (`funding`)

Premiums are signed Q32.32 (`i64`); rates and indices are signed Q64.64
(`i128`), a rate being per 9,000 slots. One basis point is `floor(2^32 / 10_000)`
in Q32.32 and `floor(2^64 / 10_000)` in Q64.64. Errors are `FundingError`.

- `clamp_funding_premium_q32(p)` returns 0 when `|p|` is below 5 bps, otherwise
  `p` clamped to ±300 bps.
- `funding_rate_q64_64_from_premium_q32(p)` = `clamp(clamped(p)·2^32 / 8, ±10 bps)`;
  the division truncates toward zero.
- `funding_index_step_q64_64(index, rem, last_slot, slots, rate)`: with
  `slots = 0` it returns the inputs and `changed = false`. Otherwise
  `n = rate·slots + rem`, `delta = trunc(n / 9_000)`, the new remainder is
  `n − delta·9_000`, the index advances by `delta` and the slot by `slots`.
  The remainder carries the truncated part into the next step. At a constant
  rate, starting from a remainder with `|rem| < 9_000`, two consecutive steps
  give the same state as one step over the sum when no step overflows. The
  function does not validate the supplied remainder.
- `funding_pnl_for_signed_size_values(size, entry, current, carry)`: a zero
  size returns `(0, carry)`. Otherwise `n = −size·(current − entry) + carry`,
  the PnL is `floor(n / 2^64)` and the new carry is `n − pnl·2^64`, in
  `[0, 2^64)`. A long pays when the index rises. The PnL must fit `i64`.
- `div_i128_floor` rounds toward negative infinity; `div_i128_trunc_toward_zero`
  toward zero. Both reject a zero divisor and `i128::MIN / −1`.
- `price_pnl_for_quote_notional(position, close_abs, entry, exec)` rejects any
  zero argument with `InvalidInput`. It returns
  `floor((exec − entry)·c / entry)` with `c = ±close_abs` by the sign of
  `position`, clamped above to `i64::MAX`; a result below `i64::MIN` is
  `Overflow`.

Every intermediate uses checked arithmetic and returns `Overflow` otherwise.

## Margin (`margin`)

Rates are basis points. Errors are `MarginError`.

- `margin_requirement_ceil(notional, bps)` = `ceil(notional·bps / 10_000)`; a
  result above `u64::MAX` is `Overflow`. `initial_margin_requirement` is the
  same function. Neither validates the rate.
- `validate_requested_initial_margin(requested, market)` accepts
  `requested >= market` up to 5,000 bps, and `requested = market` above it.
- `margin_pair_admissible(im, mm, staleness_s, confidence_bps)` is true for
  `(500, 250)`, and for `(200, 100)` when `0 < staleness_s <= 2` and
  `0 < confidence_bps <= 100`.
- `is_margin_pair_admissible(im, mm, oracle_enabled, staleness_s, confidence_bps, template_version)`
  also requires `im <= 10_000` and `mm < im`, and for `(200, 100)` an enabled
  oracle and `template_version >= 1`.

## Exposure caps (`discovery`)

Amounts are quote units × 10^6. Errors are `DiscoveryError`. Cap pairs are
`(position cap, market cap)`.

| Function | Result |
| --- | --- |
| `live_point_cap(depth)`, `live_point_market_cap(depth)` | `3·depth`, `10·depth`, saturating |
| `sampled_caps(floor)` | `(floor·1_000 / 10_000, floor·2_500 / 10_000)`; each product saturates at `u128::MAX` before the division |
| `secure_caps(floor)` | `(25·floor, 35·floor)`, saturating |
| `point_caps(floor, secure_lift)` | `secure_caps` when lifted, else `sampled_caps` |
| `depth_multiple_bps(rows)` | 2,500 for a single contributing row, else 10,000 |
| `depth_budget(depth, multiple)` | `min(50_000·10^6, depth·multiple / 10_000)` |
| `floor_cap(floor, deviation_bps, safety)` | `floor·10_000 / deviation_bps / safety`, each division rounding down |
| `closeout_notional_cap(oi_bps, basis)` | `basis·oi_bps / 10_000`; zero is `InvalidConfig` |

- `sampled_floor_usdc(floor, lower_q64, decimals)` =
  `floor(floor·lower_q64·10^6 / (10^decimals·2^64))`. It requires
  `decimals <= 18`, a nonzero price, a numerator below `2^256` and a result
  that fits `u128`.
- `floor_cap` rejects a zero floor, deviation or safety factor.
- `validate_live_notional(oi_bps, free_equity, kappa_bps, policy_max, post_abs)`
  accepts iff `0 < cap` and `post_abs <= cap`, with
  `cap = min(2^44, policy_max, free_equity·kappa_bps / 10_000, closeout_notional_cap(oi_bps, free_equity))`.
  Errors are checked in this order: `Overflow` from the vault or closeout
  product or from a closeout cap above `u64::MAX`, `InvalidConfig` for a zero
  closeout cap, then `BoundsExceeded`.
- A `Support` is an optional `(start, end)` pair in seconds; `POINT` tags a
  current-point leg, which has no interval. `meet(a, b)` returns
  `(max(start), min(end))` without checking that the result is a valid
  interval; `None` is its identity.
- `overlap(legs)` ignores point legs and returns `POINT` when every leg is a
  point. The sampled rule applies when a non-point leg is flagged sampled and
  there is more than one non-point leg or at least one point leg. Under that
  rule an unsupported non-point leg, or a common interval shorter than 120
  seconds or inverted, is `InvalidInput`. Without it, an unsupported non-point
  leg gives `None` and the raw `meet` of the supports is returned otherwise.

## Free atoms (`solvency`)

`LedgerView::free_user_atoms_total()` =
`recognized − (seed + allocated + backing + claims_payable)`, in raw atoms.
A vault balance below `recognized` or a nonfree total above `recognized`
returns `InvalidAccountData`; overflow of the nonfree sum returns
`ArithmeticOverflow`. Vault atoms above `recognized` are not counted.

## Boundary

No function validates a signer, account owner, price feed, token transfer or
account layout. Changing a unit, rounding direction, constant, envelope, error
variant or funding sign changes this contract and must update this document,
the conformance vectors and the affected harnesses.
