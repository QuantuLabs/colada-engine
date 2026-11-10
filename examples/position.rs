//! One long position from open to close, using the library's formulas.
//!
//! Run with `cargo run --example position`.

use colada_engine::{funding, margin, math};

fn main() {
    // Open a 1,000.00 long at 5% initial margin. Quote amounts carry six decimals.
    let notional: u64 = 1_000_000_000;
    let initial_margin = margin::initial_margin_requirement(notional, 500).unwrap();

    // Collateral is a nine-decimal token priced between 149.5 and 150.5 (Q64.64).
    let q64 = |tenths: u128| (tenths << 64) / 10;
    let prices = math::checked_price_bounds(q64(1_495), q64(1_505)).unwrap();
    let free_atoms: u64 = 100 * 1_000_000_000;
    let open = math::open_pre_haircut_requirement(initial_margin, prices, 9, free_atoms).unwrap();

    // The market trades 20 bps above its index for 9,000 slots.
    let premium_q32 = ((1_i64 << 32) / 10_000) * 20;
    let rate = funding::funding_rate_q64_64_from_premium_q32(premium_q32).unwrap();
    let step = funding::funding_index_step_q64_64(0, 0, 0, 9_000, rate).unwrap();

    // Close 2% higher: price PnL and funding in one result, capped by the
    // allocation on the loss side and by the notional on the profit side.
    let (net, capped) = math::funded_net_and_capped(
        notional as i64,
        100_000_000,
        102_000_000,
        0,
        step.cumulative_funding_index_q64_64,
        initial_margin,
        true,
    )
    .unwrap();

    println!("initial margin        {initial_margin} (quote x 10^6)");
    println!("collateral to lock    {} atoms", open.required_allocation_atoms);
    println!("funding index delta   {}", step.cumulative_funding_index_q64_64);
    println!("net PnL after funding {net} (capped: {capped})");
}
