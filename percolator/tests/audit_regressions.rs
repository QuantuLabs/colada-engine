use colada::*;

fn new_test_engine(params: RiskParams) -> Box<RiskEngine> {
    let layout = std::alloc::Layout::new::<RiskEngine>();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) as *mut RiskEngine };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }

    unsafe {
        (*ptr).init_in_place(params).unwrap();
        Box::from_raw(ptr)
    }
}

fn default_params() -> RiskParams {
    RiskParams {
        warmup_period_slots: 100,
        maintenance_margin_bps: 500,
        initial_margin_bps: 1000,
        trading_fee_bps: 0,
        max_accounts: 1000,
        max_exec_deviation_bps: 1_000,
        new_account_fee: U128::new(0),
        risk_reduction_threshold: U128::new(0),
        maintenance_fee_per_slot: U128::new(0),
        max_crank_staleness_slots: u64::MAX,
        max_funding_rate_bps_per_slot: ABSOLUTE_MAX_FUNDING_RATE_BPS_PER_SLOT,
        liquidation_fee_bps: 50,
        liquidation_fee_cap: U128::new(100_000),
        liquidation_buffer_bps: 100,
        min_liquidation_abs: U128::new(100_000),
    }
}

struct DiscountMatcher;
impl MatchingEngine for DiscountMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: oracle_price - 10_000_000,
            size,
        })
    }
}

struct ExtremePriceMatcher;
impl MatchingEngine for ExtremePriceMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution> {
        Ok(TradeExecution {
            price: oracle_price / 2,
            size,
        })
    }
}

#[test]
fn funding_rate_validation_rejects_poison_and_extremes() {
    let mut params = default_params();
    params.max_funding_rate_bps_per_slot = 10;
    let mut engine = new_test_engine(params);

    assert!(engine.set_funding_rate_for_next_interval(i64::MIN).is_err());
    assert_eq!(engine.funding_rate_bps_per_slot_last, 0);
    assert!(engine.set_funding_rate_for_next_interval(11).is_err());
    assert!(engine.set_funding_rate_for_next_interval(-11).is_err());

    engine.set_funding_rate_for_next_interval(10).unwrap();
    assert_eq!(engine.funding_rate_bps_per_slot_last, 10);
}

#[test]
fn set_owner_requires_current_owner() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();
    let alice = [1u8; 32];
    let bob = [2u8; 32];
    let mallory = [3u8; 32];

    assert!(engine.set_owner(user, mallory, alice).is_err());
    engine.set_owner(user, alice, alice).unwrap();
    assert_eq!(engine.accounts[user as usize].owner, alice);

    assert!(engine.set_owner(user, mallory, bob).is_err());
    engine.set_owner(user, alice, bob).unwrap();
    assert_eq!(engine.accounts[user as usize].owner, bob);
}

#[test]
fn matcher_exec_price_deviation_is_rejected() {
    let mut engine = new_test_engine(default_params());
    let lp = engine.add_lp([1u8; 32], [2u8; 32], 0).unwrap();
    let user = engine.add_user(0).unwrap();
    engine.deposit(lp, 1_000_000_000, 0).unwrap();
    engine.deposit(user, 1_000_000_000, 0).unwrap();

    let result = engine.execute_trade(&ExtremePriceMatcher, lp, user, 0, 1_000_000, 1_000);

    assert_eq!(result, Err(RiskError::InvalidMatchingEngine));
    assert!(engine.accounts[user as usize].position_size.is_zero());
    assert!(engine.accounts[lp as usize].position_size.is_zero());
}

#[test]
fn max_accounts_is_sanitized_to_compile_time_ceiling() {
    let mut params = default_params();
    params.max_accounts = MAX_ACCOUNTS as u64 + 1;
    let engine = new_test_engine(params);

    assert_eq!(engine.params.max_accounts, MAX_ACCOUNTS as u64);
}

#[test]
fn funding_with_rate_is_anti_retroactive() {
    let mut params = default_params();
    params.max_funding_rate_bps_per_slot = 10;
    let mut engine = new_test_engine(params);

    engine.accrue_funding_with_rate(10, 1_000_000, 10).unwrap();

    assert_eq!(
        engine.funding_index_qpb_e6.get(),
        0,
        "new rate must not apply to the elapsed interval"
    );
    assert_eq!(engine.last_funding_slot, 10);
    assert_eq!(engine.funding_rate_bps_per_slot_last, 10);

    engine.accrue_funding(11, 1_000_000).unwrap();
    assert_eq!(
        engine.funding_index_qpb_e6.get(),
        1_000,
        "stored rate should apply only from the next interval"
    );
}

#[test]
fn liquidation_rechecks_after_fee() {
    let mut params = default_params();
    params.maintenance_margin_bps = 500;
    params.liquidation_buffer_bps = 100;
    params.liquidation_fee_bps = 50;
    params.liquidation_fee_cap = U128::new(100_000);
    params.min_liquidation_abs = U128::new(100_000);
    let mut engine = new_test_engine(params);
    let user = engine.add_user(0).unwrap();

    engine.set_capital(user as usize, 606_000);
    engine.vault = U128::new(606_000);
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.total_open_interest = U128::new(10_000_000);

    assert!(engine.liquidate_at_oracle(user, 0, 950_000).unwrap());

    let account = &engine.accounts[user as usize];
    assert!(
        account.position_size.is_zero() || engine.is_above_maintenance_margin_mtm(account, 950_000),
        "liquidation must not return success with an unsafe remnant"
    );
}

#[test]
fn fresh_trade_pnl_cannot_consume_stale_warmup_budget() {
    let mut engine = new_test_engine(default_params());
    let lp = engine.add_lp([1u8; 32], [2u8; 32], 0).unwrap();
    let user = engine.add_user(0).unwrap();
    engine.deposit(lp, 1_000_000_000, 0).unwrap();
    engine.deposit(user, 1_000_000_000, 0).unwrap();

    engine.accounts[user as usize].warmup_slope_per_step = U128::new(100);
    engine.accounts[user as usize].warmup_started_at_slot = 0;
    let capital_before = engine.accounts[user as usize].capital.get();

    engine
        .execute_trade(&DiscountMatcher, lp, user, 50, 100_000_000, 1_000)
        .unwrap();

    assert_eq!(
        engine.accounts[user as usize].capital.get(),
        capital_before,
        "fresh trade PnL must not settle to capital in the same slot"
    );
    assert!(engine.accounts[user as usize].pnl.get() > 0);
    assert_eq!(engine.accounts[user as usize].warmup_started_at_slot, 50);
}

#[test]
fn i128_min_abs_and_neg_are_saturating() {
    let min = I128::new(i128::MIN);

    assert_eq!(min.abs().get(), i128::MAX);
    assert_eq!((-min).get(), i128::MAX);
}

#[test]
fn public_slots_are_monotonic() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();

    engine.deposit(user, 1_000, 10).unwrap();
    assert_eq!(engine.current_slot, 10);
    assert!(engine.deposit(user, 1, 9).is_err());
    assert_eq!(engine.current_slot, 10);
}

#[test]
fn staleness_window_is_below_warmup_period() {
    let mut params = default_params();
    params.warmup_period_slots = 100;
    params.max_crank_staleness_slots = u64::MAX;
    let engine = new_test_engine(params);

    assert_eq!(engine.params.max_crank_staleness_slots, 99);
    assert_eq!(engine.max_crank_staleness_slots, 99);
}

#[test]
fn healthy_dust_position_survives_crank() {
    let mut params = default_params();
    params.min_liquidation_abs = U128::new(100_000);
    params.risk_reduction_threshold = U128::new(1_000);
    let mut engine = new_test_engine(params);
    let lp = engine.add_lp([1u8; 32], [2u8; 32], 0).unwrap();
    let user = engine.add_user(0).unwrap();

    engine.deposit(lp, 50_000, 0).unwrap();
    engine.deposit(user, 10_000, 0).unwrap();
    engine.accounts[user as usize].position_size = I128::new(50_000);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.accounts[lp as usize].position_size = I128::new(-50_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;
    engine.total_open_interest = U128::new(100_000);
    engine.insurance_fund.balance = U128::new(2_000);

    engine
        .keeper_crank(u16::MAX, 1, 1_000_000, 0, false)
        .unwrap();

    assert_eq!(engine.accounts[user as usize].position_size.get(), 50_000);
}

#[test]
fn conservation_detects_corrupt_stored_aggregates() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();
    engine.deposit(user, 1_000, 0).unwrap();

    engine.c_tot = U128::new(0);
    assert!(!engine.check_conservation(1_000_000));

    engine.recompute_aggregates();
    engine.accounts[user as usize].pnl = I128::new(100);
    engine.pnl_pos_tot = U128::new(0);
    assert!(!engine.check_conservation(1_000_000));
}

#[test]
fn init_sanitizes_zero_warmup_and_rejects_reinit() {
    let mut params = default_params();
    params.warmup_period_slots = 0;
    let mut engine = new_test_engine(params);
    assert_eq!(engine.params.warmup_period_slots, 1);

    let user = engine.add_user(0).unwrap();
    engine.deposit(user, 1, 0).unwrap();
    assert!(engine.init_in_place(default_params()).is_err());
}

#[test]
fn invalid_account_kind_is_data_not_ub() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();
    engine.deposit(user, 1_000, 0).unwrap();

    engine.accounts[user as usize].kind = 7;

    assert!(!engine.accounts[user as usize].has_valid_kind());
    assert!(!engine.check_conservation(1_000_000));
}

#[test]
fn liquidation_overflow_sizes_to_full_close() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].capital = U128::new(u128::MAX / 10_000_000_000 + 1);
    engine.accounts[user as usize].position_size = I128::new(1_000_000);
    engine.accounts[user as usize].entry_price = 1;

    let (close_abs, full_close) =
        engine.compute_liquidation_close_amount(&engine.accounts[user as usize], 1);

    assert_eq!(close_abs, 1_000_000);
    assert!(full_close);
}

#[test]
fn reserved_pnl_setter_enforces_backing() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();
    engine.accounts[user as usize].pnl = I128::new(100);
    engine.recompute_aggregates();

    engine.set_reserved_pnl(user, 40).unwrap();
    assert_eq!(engine.accounts[user as usize].reserved_pnl, 40);
    assert!(engine.set_reserved_pnl(user, 101).is_err());
}

#[test]
fn open_interest_desync_is_not_silently_saturated() {
    let mut engine = new_test_engine(default_params());
    let lp = engine.add_lp([1u8; 32], [2u8; 32], 0).unwrap();
    let user = engine.add_user(0).unwrap();
    engine.deposit(lp, 1_000_000, 0).unwrap();
    engine.deposit(user, 1_000_000, 0).unwrap();

    engine.accounts[user as usize].position_size = I128::new(1_000);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.accounts[lp as usize].position_size = I128::new(-1_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;
    engine.total_open_interest = U128::ZERO;

    let result = engine.execute_trade(&NoOpMatcher, lp, user, 0, 1_000_000, -1_000);

    assert!(matches!(result, Err(RiskError::Overflow)));
}

#[test]
fn crank_does_not_forgive_caller_fee_debt() {
    let mut params = default_params();
    params.maintenance_fee_per_slot = U128::new(10);
    let mut engine = new_test_engine(params);
    let user = engine.add_user(0).unwrap();
    engine.deposit(user, 1_000, 0).unwrap();

    let outcome = engine.keeper_crank(user, 10, 1_000_000, 0, false).unwrap();

    assert_eq!(outcome.slots_forgiven, 0);
    assert_eq!(engine.accounts[user as usize].capital.get(), 900);
    assert_eq!(engine.insurance_fund.balance.get(), 100);
}

#[test]
fn liquidation_mark_gain_restarts_warmup() {
    let mut params = default_params();
    params.liquidation_fee_bps = 0;
    let mut engine = new_test_engine(params);
    let user = engine.add_user(0).unwrap();
    let lp = engine.add_lp([1u8; 32], [2u8; 32], 0).unwrap();

    engine.current_slot = 90;
    engine.set_capital(user as usize, 500);
    engine.set_pnl(user as usize, 1_000);
    engine.accounts[user as usize].position_size = I128::new(10_000_000);
    engine.accounts[user as usize].entry_price = 1_000_000;
    engine.accounts[user as usize].warmup_slope_per_step = U128::new(10);
    engine.accounts[user as usize].warmup_started_at_slot = 0;

    engine.set_capital(lp as usize, 10_000_000);
    engine.accounts[lp as usize].position_size = I128::new(-10_000_000);
    engine.accounts[lp as usize].entry_price = 1_000_000;
    engine.total_open_interest = U128::new(20_000_000);
    engine.vault = U128::new(10_000_500 + 10_000);

    assert!(engine.liquidate_at_oracle(user, 90, 1_000_001).unwrap());
    assert_eq!(
        engine.accounts[user as usize].capital.get(),
        500,
        "mark gain at liquidation must restart warmup before conversion"
    );
}

#[test]
fn init_in_place_rejects_maintenance_above_initial_margin() {
    let mut params = default_params();
    params.maintenance_margin_bps = 2_000;
    params.initial_margin_bps = 1_000;
    let layout = std::alloc::Layout::new::<RiskEngine>();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) as *mut RiskEngine };
    assert!(!ptr.is_null());
    let mut engine = unsafe { Box::from_raw(ptr) };
    assert_eq!(engine.init_in_place(params), Err(RiskError::Overflow));

    params.maintenance_margin_bps = 1_000;
    assert!(engine.init_in_place(params).is_ok());
}

#[test]
#[should_panic(expected = "maintenance_margin_bps must not exceed initial_margin_bps")]
fn new_rejects_maintenance_above_initial_margin() {
    let mut params = default_params();
    params.maintenance_margin_bps = 2_000;
    params.initial_margin_bps = 1_000;
    let _ = RiskEngine::new(params);
}

#[test]
fn deposit_fee_credits_rejects_amount_beyond_i128() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();

    for amount in [1u128 << 127, u128::MAX] {
        assert_eq!(
            engine.deposit_fee_credits(user, amount, 0),
            Err(RiskError::Overflow)
        );
    }
    engine.deposit_fee_credits(user, i128::MAX as u128, 0).unwrap();
    assert_eq!(engine.deposit_fee_credits(user, 1, 0), Err(RiskError::Overflow));
    assert_eq!(engine.accounts[user as usize].fee_credits.get(), i128::MAX);
    assert_eq!(engine.vault.get(), i128::MAX as u128);
    assert_eq!(engine.insurance_fund.balance.get(), i128::MAX as u128);
}

#[test]
fn gc_keeps_account_with_prepaid_fee_credits() {
    let mut engine = new_test_engine(default_params());
    let user = engine.add_user(0).unwrap();
    engine.deposit_fee_credits(user, 1, 0).unwrap();

    assert_eq!(engine.garbage_collect_dust(), 0);
    assert!(engine.is_used(user as usize));
    assert_eq!(engine.accounts[user as usize].fee_credits.get(), 1);
}
