use std::collections::VecDeque;
use std::future::{pending, ready};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use candid::{Decode, Encode, Principal};
use icp_events::{EventKind, TaskOutcome};
use icp_project::calls::{Authority, Call, CallError, CanisterCalls};
use icp_project::operations::task::{PurchasePhase, Reporter, Task};
use url::Url;

use super::interface::{
    CREATE_ORDER_METHOD, CardOrderStatus, CreateOrderArgs, CreateOrderResult, CreatedOrder,
    Destination, MAX_ORDER_USD_CENTS, MIN_ORDER_USD_CENTS, ORDER_STATUS_METHOD, OrderDestination,
    OrderError, OrderErrorKind, OrderStatusView, PAYMENT_LINK_URL, XDR_RATE_STATUS_METHOD,
    XdrRateStatus,
};
use super::money::{Currency, Money};
use super::pricing::ledger_cycles;
use super::purchase::*;

const BACKEND: &str = "2bwus-iiaaa-aaaac-bee3a-cai";
const BUYER: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

/// $1.3520 an XDR.
const RATE: u64 = 13_520;

fn principal(text: &str) -> Principal {
    Principal::from_text(text).unwrap()
}

fn gateway() -> Gateway {
    Gateway {
        canister: principal(BACKEND),
        payment_link: Url::parse(PAYMENT_LINK_URL).unwrap(),
    }
}

fn usd(cents: u64) -> Money {
    Money::from_minor_units(cents, Currency::Usd)
}

fn rate() -> XdrRateStatus {
    XdrRateStatus {
        xdr_usd_x10000: RATE,
        stale: false,
        age_seconds: Some(120),
    }
}

/// Far enough ahead that no test trips the deadline by accident.
const FAR_FUTURE: u64 = u64::MAX / 2;

fn created(id: &str, cents: u64) -> CreateOrderResult {
    CreateOrderResult::Ok(CreatedOrder {
        order_id: id.to_owned(),
        client_reference_id: format!("o_{id}"),
        amount_usd_cents: cents,
        net_usd_cents: super::pricing::net_usd_cents(cents),
        expires_at: FAR_FUTURE,
    })
}

fn refused(code: &str) -> CreateOrderResult {
    CreateOrderResult::Err(OrderError {
        code: code.to_owned(),
        message: "as the backend puts it".to_owned(),
    })
}

fn view(id: &str, status: CardOrderStatus) -> OrderStatusView {
    let paid = !matches!(status, CardOrderStatus::AwaitingPayment);
    OrderStatusView {
        order_id: id.to_owned(),
        status: Some(status),
        amount_usd_cents: 2500,
        net_usd_cents: 2398,
        paid_usd_cents: paid.then_some(2500),
        cycles_delivered: (status == CardOrderStatus::Delivered).then_some(17_736_486_390_532),
        amount_refunded_usd_cents: match status {
            CardOrderStatus::Refunded => 2500,
            _ => 0,
        },
        detail: None,
        expires_at: (!paid).then_some(FAR_FUTURE),
        destination: Some(Destination::Ledger(principal(BUYER))),
    }
}

fn unanswered() -> CallError {
    CallError::unanswered(
        principal(BACKEND),
        ORDER_STATUS_METHOD,
        std::io::Error::other("connection reset"),
    )
}

fn rejected(message: &str) -> CallError {
    CallError::Rejected {
        canister: principal(BACKEND),
        method: ORDER_STATUS_METHOD.to_owned(),
        code: Some("IC0503".to_owned()),
        message: message.to_owned(),
    }
}

/// A CyclePay backend whose replies are scripted per method.
#[derive(Default)]
struct FakeBackend {
    /// What `get_xdr_rate_status` answers; `None` answers a rate of zero.
    rate: Option<XdrRateStatus>,
    /// One reply per `create_order` call, in order.
    create: Mutex<VecDeque<CreateOrderResult>>,
    /// One reply per `order_status` poll, in order. An `Err` is the network
    /// failing to answer, or rejecting.
    status: Mutex<VecDeque<Result<Option<OrderStatusView>, CallError>>>,
    /// Every call made, as (method, candid argument).
    calls_seen: Mutex<Vec<(String, Vec<u8>)>>,
    /// Rejects every call as if the canister did not exist.
    absent: bool,
    /// Never answers `order_status`, as a backend behind a stalled connection.
    stall_polls: bool,
}

impl FakeBackend {
    fn priced() -> Self {
        Self {
            rate: Some(rate()),
            ..Self::default()
        }
    }

    fn polls(self, replies: Vec<Result<Option<OrderStatusView>, CallError>>) -> Self {
        *self.status.lock().unwrap() = replies.into();
        self
    }

    fn creates(self, replies: Vec<CreateOrderResult>) -> Self {
        *self.create.lock().unwrap() = replies.into();
        self
    }

    fn arg_of(&self, method: &str) -> Vec<u8> {
        self.calls_seen
            .lock()
            .unwrap()
            .iter()
            .find(|(m, _)| m == method)
            .map(|(_, arg)| arg.clone())
            .unwrap_or_else(|| panic!("no call to {method}"))
    }

    fn count_of(&self, method: &str) -> usize {
        self.calls_seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m == method)
            .count()
    }

    fn record(&self, call: &Call) -> Result<(), CallError> {
        assert_eq!(call.canister, principal(BACKEND));
        self.calls_seen
            .lock()
            .unwrap()
            .push((call.method.clone(), call.arg.clone()));
        if self.absent {
            return Err(CallError::Rejected {
                canister: call.canister,
                method: call.method.clone(),
                code: Some("IC0301".to_owned()),
                message: format!("Canister {BACKEND} not found"),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl CanisterCalls for FakeBackend {
    fn caller(&self) -> Principal {
        principal(BUYER)
    }

    async fn update(&self, call: Call) -> Result<Vec<u8>, CallError> {
        self.record(&call)?;
        match call.method.as_str() {
            CREATE_ORDER_METHOD => {
                let reply = self
                    .create
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("ran out of scripted create_order replies");
                Ok(Encode!(&reply).unwrap())
            }
            other => panic!("unexpected update {other}"),
        }
    }

    async fn query(&self, call: Call) -> Result<Vec<u8>, CallError> {
        self.record(&call)?;
        match call.method.as_str() {
            XDR_RATE_STATUS_METHOD => {
                let status = self.rate.clone().unwrap_or(XdrRateStatus {
                    xdr_usd_x10000: 0,
                    stale: true,
                    age_seconds: None,
                });
                Ok(Encode!(&status).unwrap())
            }
            ORDER_STATUS_METHOD => {
                if self.stall_polls {
                    pending::<()>().await;
                }
                let reply = self
                    .status
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("ran out of scripted order_status replies")?;
                Ok(Encode!(&reply).unwrap())
            }
            other => panic!("unexpected query {other}"),
        }
    }

    async fn metadata_section(
        &self,
        _canister: Principal,
        _path: &str,
        _authority: Authority,
    ) -> Result<Option<Vec<u8>>, CallError> {
        unimplemented!()
    }

    async fn controllers(&self, _canister: Principal) -> Result<Option<Vec<Principal>>, CallError> {
        unimplemented!()
    }

    async fn module_hash(&self, _canister: Principal) -> Result<Option<Vec<u8>>, CallError> {
        unimplemented!()
    }

    async fn subnet_of(&self, _canister: Principal) -> Result<Principal, CallError> {
        unimplemented!()
    }

    async fn subnet_uses_engine_operator(&self, _subnet: Principal) -> Result<bool, CallError> {
        unimplemented!()
    }
}

fn fast() -> WaitOptions {
    WaitOptions {
        poll_interval: Duration::from_millis(1),
        ..WaitOptions::default()
    }
}

// --- quote -----------------------------------------------------------------

#[tokio::test]
async fn quote_spend_reads_the_rate_and_prices_at_it() {
    let fake = FakeBackend::priced();
    let q = quote_target(&fake, &gateway(), Target::Spend(usd(2500)))
        .await
        .unwrap();
    assert_eq!(q.amount, usd(2500));
    assert_eq!(q.fee, usd(102));
    assert_eq!(q.net, usd(2398));
    assert_eq!(q.cycles, ledger_cycles(2500, RATE));
    assert_eq!(q.rate.xdr_usd_x10000, RATE);
    assert!(!q.rate.stale);
    assert_eq!(q.rate.age, Some(Duration::from_secs(120)));
    assert_eq!(q.rate.usd_per_trillion(), "1.3520 USD");
    assert_eq!(fake.count_of(XDR_RATE_STATUS_METHOD), 1);
}

#[tokio::test]
async fn quote_outside_the_range_is_refused_before_any_order() {
    let fake = FakeBackend::priced();
    for cents in [MIN_ORDER_USD_CENTS - 1, MAX_ORDER_USD_CENTS + 1] {
        let err = quote_target(&fake, &gateway(), Target::Spend(usd(cents)))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, CyclesPurchaseError::AmountOutOfRange { amount, min, max }
                if *amount == usd(cents) && *min == usd(100) && *max == usd(20_000)),
            "{err}"
        );
    }
    assert_eq!(
        quote_at(
            Rate {
                xdr_usd_x10000: RATE,
                stale: false,
                age: None
            },
            Target::Spend(usd(50))
        )
        .unwrap_err()
        .to_string(),
        "amount 0.50 USD is outside the gateway's range of 1.00 USD to 200.00 USD per order"
    );
}

#[tokio::test]
async fn quote_without_rate_is_rate_unavailable() {
    let fake = FakeBackend::default();
    let err = quote_target(&fake, &gateway(), Target::Spend(usd(1000)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::RateUnavailable { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn quote_carries_a_stale_rate_through() {
    let fake = FakeBackend {
        rate: Some(XdrRateStatus {
            stale: true,
            age_seconds: None,
            ..rate()
        }),
        ..FakeBackend::default()
    };
    let q = quote_target(&fake, &gateway(), Target::Spend(usd(1000)))
        .await
        .unwrap();
    assert!(q.rate.stale);
    assert_eq!(q.rate.age, None);
}

#[tokio::test]
async fn quote_on_missing_gateway_is_gateway_not_found() {
    let fake = FakeBackend {
        absent: true,
        ..FakeBackend::default()
    };
    let err = quote_target(&fake, &gateway(), Target::Spend(usd(1000)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );
    // CyclePay's own gateway is on mainnet only, and the error says so as
    // a run with no network named does; another gateway could be anywhere.
    assert_eq!(
        err.to_string(),
        format!(
            "the cycles gateway canister {BACKEND} does not exist on this network. CyclePay \
             sells cycles on mainnet: add `-n ic`"
        )
    );
    let other = CyclesPurchaseError::GatewayNotFound {
        gateway: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
    };
    assert_eq!(
        other.to_string(),
        "the cycles gateway canister ryjl3-tyaaa-aaaaa-aaaba-cai does not exist on this network"
    );
}

// --- quote for cycles ------------------------------------------------------

fn receive(cycles: u128) -> Target {
    Target::Receive {
        cycles,
        currency: Currency::Usd,
    }
}

#[tokio::test]
async fn quote_receive_pays_the_least_that_buys_them() {
    let fake = FakeBackend::priced();
    let target = 5_000_000_000_000;
    let q = quote_target(&fake, &gateway(), receive(target))
        .await
        .unwrap();
    assert!(q.cycles >= target);
    assert!(ledger_cycles(q.amount.minor_units - 1, RATE) < target);
    assert_eq!(q.cycles, ledger_cycles(q.amount.minor_units, RATE));
    assert_eq!(q.fee.minor_units + q.net.minor_units, q.amount.minor_units);
}

#[tokio::test]
async fn quote_receive_below_the_minimum_pays_the_minimum() {
    let fake = FakeBackend::priced();
    let q = quote_target(&fake, &gateway(), receive(1)).await.unwrap();
    assert_eq!(q.amount, usd(MIN_ORDER_USD_CENTS));
    assert_eq!(q.cycles, ledger_cycles(MIN_ORDER_USD_CENTS, RATE));
}

#[tokio::test]
async fn quote_receive_beyond_the_maximum_names_what_it_buys() {
    let fake = FakeBackend::priced();
    let most = ledger_cycles(MAX_ORDER_USD_CENTS, RATE);
    let err = quote_target(&fake, &gateway(), receive(most + 1))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CyclesPurchaseError::CyclesAboveMaximum { cycles, max, buys }
            if *cycles == most + 1 && *max == usd(20_000) && *buys == most),
        "{err}"
    );
    assert!(err.to_string().contains("200.00 USD"), "{err}");
}

// --- place_order -----------------------------------------------------------

#[tokio::test]
async fn place_order_sends_the_amount_to_the_callers_ledger_account() {
    let fake = FakeBackend::default().creates(vec![created("co-1", 2500)]);
    let order = place_order(&fake, &gateway(), usd(2500)).await.unwrap();
    assert_eq!(order.id, "co-1");
    assert_eq!(order.status, OrderStatus::AwaitingPayment);
    assert_eq!(order.amount, usd(2500));
    assert_eq!(order.net, usd(2398));
    assert_eq!(order.expires_at, Some(FAR_FUTURE));
    assert_eq!(
        order.checkout_url.as_deref(),
        Some(
            format!("{PAYMENT_LINK_URL}?client_reference_id=o_co-1&prefilled_amount=2500").as_str()
        )
    );
    assert_eq!(order.paid, None);
    assert_eq!(order.cycles_delivered, None);

    let args = Decode!(&fake.arg_of(CREATE_ORDER_METHOD), CreateOrderArgs).unwrap();
    assert_eq!(
        args,
        CreateOrderArgs {
            destination: OrderDestination::Ledger,
            beneficiary: None,
            project_id: None,
            amount_usd_cents: 2500,
            confirm_system_canister: None,
        }
    );
}

/// The link carries `o_<order id>`, as CyclePay's site builds it; an order
/// made with any other reference is not handed out to be paid.
#[tokio::test]
async fn place_order_refuses_an_order_with_another_reference() {
    let CreateOrderResult::Ok(mut order) = created("co-1", 2500) else {
        unreachable!()
    };
    order.client_reference_id = "o_co-2".to_owned();
    let fake = FakeBackend::default().creates(vec![CreateOrderResult::Ok(order)]);
    let err = place_order(&fake, &gateway(), usd(2500)).await.unwrap_err();
    assert!(
        matches!(&err, CyclesPurchaseError::UnexpectedReference { id, reference, .. }
            if id == "co-1" && reference == "o_co-2"),
        "{err}"
    );
}

#[tokio::test]
async fn place_order_maps_a_refusal() {
    let fake = FakeBackend::default().creates(vec![refused("RATE_LIMITED")]);
    let err = place_order(&fake, &gateway(), usd(2500)).await.unwrap_err();
    match err {
        CyclesPurchaseError::OrderRefused { error } => {
            assert_eq!(error.kind(), OrderErrorKind::RateLimited);
            assert_eq!(error.to_string(), "RATE_LIMITED: as the backend puts it");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn place_order_on_missing_gateway_is_gateway_not_found() {
    let fake = FakeBackend {
        absent: true,
        ..FakeBackend::default()
    };
    let err = place_order(&fake, &gateway(), usd(2500)).await.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );
}

// --- get_order -------------------------------------------------------------

#[tokio::test]
async fn get_order_rebuilds_the_checkout_url_while_payable() {
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
    ]);
    let order = get_order(&fake, &gateway(), "co-1").await.unwrap();
    assert_eq!(
        order.checkout_url.as_deref(),
        Some(
            format!("{PAYMENT_LINK_URL}?client_reference_id=o_co-1&prefilled_amount=2500").as_str()
        )
    );
    let id = Decode!(&fake.arg_of(ORDER_STATUS_METHOD), String).unwrap();
    assert_eq!(id, "co-1");

    let order = get_order(&fake, &gateway(), "co-1").await.unwrap();
    assert_eq!(order.status, OrderStatus::Processing);
    assert_eq!(order.checkout_url, None);
    assert_eq!(order.paid, Some(usd(2500)));
    assert_eq!(order.expires_at, None);
}

#[tokio::test]
async fn get_order_none_is_order_not_found() {
    let fake = FakeBackend::default().polls(vec![Ok(None)]);
    let err = get_order(&fake, &gateway(), "ghost").await.unwrap_err();
    match err {
        CyclesPurchaseError::OrderNotFound { id, caller, .. } => {
            assert_eq!(id, "ghost");
            assert_eq!(caller, principal(BUYER));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn get_order_reads_a_refund() {
    let mut refunded = view("co-1", CardOrderStatus::Refunded);
    refunded.detail = Some("OVER_CAP".into());
    let fake = FakeBackend::default().polls(vec![Ok(Some(refunded))]);
    let order = get_order(&fake, &gateway(), "co-1").await.unwrap();
    assert_eq!(order.status, OrderStatus::Refunded);
    assert_eq!(order.refunded, Some(usd(2500)));
    assert_eq!(order.detail.as_deref(), Some("OVER_CAP"));
}

// --- wait_for_order --------------------------------------------------------

/// A reported phase and how it ended: `Err` carries the failure message.
type Phase = (PurchasePhase, Result<(), String>);

/// Run the wait and collect the phases it reported, each with its outcome.
async fn wait(
    fake: &FakeBackend,
    initial: Option<OrderView>,
    options: &WaitOptions,
) -> (Result<OrderView, CyclesPurchaseError>, Vec<Phase>) {
    let (reporter, mut events) = icp_events::channel();
    let result = wait_for_order(
        fake,
        &gateway(),
        "co-1",
        initial,
        options,
        pending(),
        &reporter,
    )
    .await;
    drop(reporter);
    let mut phases = Vec::new();
    while let Some(event) = events.recv().await {
        match event.kind {
            EventKind::TaskStarted {
                task: Task::CyclesPurchase(task),
                ..
            } => phases.push((task.phase, None)),
            EventKind::TaskCompleted { outcome } => {
                let (_, slot) = phases.last_mut().expect("completed before started");
                *slot = Some(match outcome {
                    TaskOutcome::Succeeded { .. } => Ok(()),
                    TaskOutcome::Failed { message, .. } => Err(message),
                    TaskOutcome::Skipped { reason } => Err(reason),
                    TaskOutcome::Stopped { message } => Err(message),
                });
            }
            other => panic!("unexpected event {other:?}"),
        }
    }
    (
        result,
        phases
            .into_iter()
            .map(|(phase, outcome)| (phase, outcome.expect("phase never finished")))
            .collect(),
    )
}

fn succeeded(phase: PurchasePhase) -> Phase {
    (phase, Ok(()))
}

fn failed(phase: PurchasePhase, message: &str) -> Phase {
    (phase, Err(message.to_owned()))
}

async fn initial(status: CardOrderStatus) -> OrderView {
    let fake = FakeBackend::default().polls(vec![Ok(Some(view("co-1", status)))]);
    get_order(&fake, &gateway(), "co-1").await.unwrap()
}

#[tokio::test]
async fn wait_happy_path_awaiting_processing_delivered() {
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Ok(Some(view("co-1", CardOrderStatus::PaymentPending))),
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let (result, phases) = wait(&fake, None, &fast()).await;
    let order = result.unwrap();
    assert_eq!(order.status, OrderStatus::Delivered);
    assert_eq!(order.cycles_delivered, Some(17_736_486_390_532));
    // Pending and processing are one leg: the card has been charged.
    assert_eq!(
        phases,
        vec![
            succeeded(PurchasePhase::AwaitingPayment),
            succeeded(PurchasePhase::Delivering),
        ]
    );
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 5);
}

#[tokio::test]
async fn wait_starts_in_the_initial_orders_phase() {
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let paid = initial(CardOrderStatus::Processing).await;
    let (result, phases) = wait(&fake, Some(paid), &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
    // Already paid: no "awaiting payment" leg is reported.
    assert_eq!(phases, vec![succeeded(PurchasePhase::Delivering)]);
}

/// Delivered between two polls, the order is never seen paying; its wait
/// still ends with both legs ticked off.
#[tokio::test]
async fn wait_ticks_off_both_legs_of_a_delivery_seen_only_at_its_end() {
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let (result, phases) = wait(&fake, None, &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
    assert_eq!(
        phases,
        vec![
            succeeded(PurchasePhase::AwaitingPayment),
            succeeded(PurchasePhase::Delivering),
        ]
    );
}

#[tokio::test]
async fn wait_returns_every_settled_status_as_ok() {
    for (status, expected) in [
        (CardOrderStatus::Refunding, OrderStatus::Refunding),
        (CardOrderStatus::Refunded, OrderStatus::Refunded),
        (CardOrderStatus::RefundOwed, OrderStatus::RefundOwed),
        (CardOrderStatus::NeedsReview, OrderStatus::NeedsReview),
    ] {
        let fake = FakeBackend::default().polls(vec![Ok(Some(view("co-1", status)))]);
        let (result, phases) = wait(&fake, None, &fast()).await;
        assert_eq!(result.unwrap().status, expected);
        // An answer, but not the one hoped for: the leg is shown as failed.
        assert_eq!(
            phases,
            vec![failed(
                PurchasePhase::AwaitingPayment,
                &format!("order {expected}")
            )]
        );
    }
}

#[tokio::test]
async fn wait_tolerates_transient_errors() {
    let fake = FakeBackend::default().polls(vec![
        Err(unanswered()),
        Err(unanswered()),
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Err(unanswered()),
        Err(unanswered()),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let options = WaitOptions {
        max_consecutive_transient: 2,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
}

/// While CyclePay upgrades its backend, every call is refused as stopped
/// (IC0509 while it stops, IC0508 once stopped) for a few minutes, and a
/// boundary node may answer 429 or 5xx. Nothing about the order changed.
#[tokio::test]
async fn wait_rides_out_a_backend_upgrade_and_a_busy_boundary_node() {
    use ic_agent::AgentError;
    use ic_agent::agent::agent_error::HttpErrorPayload;
    let stopped = |code: &str| CallError::Rejected {
        canister: principal(BACKEND),
        method: ORDER_STATUS_METHOD.to_owned(),
        code: Some(code.to_owned()),
        message: "Canister is stopped".to_owned(),
    };
    let http = |status| {
        CallError::failed(
            principal(BACKEND),
            ORDER_STATUS_METHOD,
            AgentError::HttpError(HttpErrorPayload {
                status,
                content_type: None,
                content: b"try again".to_vec(),
            }),
        )
    };
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Err(stopped("IC0509")),
        Err(stopped("IC0508")),
        Err(http(503)),
        Err(http(429)),
        Err(http(502)),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let (result, _) = wait(&fake, None, &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);

    // A 4xx other than those is an answer, and ends the wait.
    let fake = FakeBackend::default().polls(vec![Err(http(403))]);
    let (result, _) = wait(&fake, None, &fast()).await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::GetOrder { .. }
    ));
}

#[tokio::test]
async fn wait_gives_up_after_consecutive_transient_errors() {
    let fake = FakeBackend::default().polls(vec![
        Err(unanswered()),
        Err(unanswered()),
        Err(unanswered()),
    ]);
    let options = WaitOptions {
        max_consecutive_transient: 2,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    let err = result.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::WaitUnanswered { .. }),
        "{err}"
    );
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 3);
}

#[tokio::test]
async fn wait_fails_fast_on_non_transient_error() {
    let fake = FakeBackend::default().polls(vec![Err(rejected("canister trapped"))]);
    let (result, _) = wait(&fake, None, &fast()).await;
    let err = result.unwrap_err();
    assert!(matches!(err, CyclesPurchaseError::GetOrder { .. }), "{err}");
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 1);
}

#[tokio::test]
async fn wait_not_found_before_the_deadline_is_order_not_found() {
    let fake = FakeBackend::default().polls(vec![Ok(None)]);
    let (result, phases) = wait(&fake, None, &fast()).await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::OrderNotFound { .. }
    ));
    assert_eq!(
        phases,
        vec![failed(PurchasePhase::AwaitingPayment, "order not found")]
    );

    // Known and payable, then gone for longer than a replica lags: not an
    // expiry, so not the caller's. That takes its own, short budget, not
    // the ten minutes an unanswered network is given.
    let mut polls = vec![Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment)))];
    polls.extend((0..=WaitOptions::default().max_consecutive_missing).map(|_| Ok(None)));
    let fake = FakeBackend::default().polls(polls);
    let (result, _) = wait(&fake, None, &fast()).await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::OrderNotFound { .. }
    ));
    assert_eq!(
        fake.count_of(ORDER_STATUS_METHOD),
        2 + WaitOptions::default().max_consecutive_missing as usize
    );
}

/// Right after `create_order` a poll can reach a replica that has not seen
/// the order yet; the wait goes on rather than calling the order not found.
#[tokio::test]
async fn wait_takes_a_known_orders_null_for_a_replica_behind() {
    let fake = FakeBackend::default().polls(vec![
        Ok(None),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let initial = initial(CardOrderStatus::AwaitingPayment).await;
    let (result, _) = wait(&fake, Some(initial), &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
}

/// A state CyclePay adds after this CLI was built does not stop the wait.
#[tokio::test]
async fn wait_goes_on_through_a_state_it_does_not_know() {
    let mut unknown = view("co-1", CardOrderStatus::Processing);
    unknown.status = None;
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
        Ok(Some(unknown)),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let (result, phases) = wait(&fake, None, &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
    assert_eq!(
        phases,
        vec![
            succeeded(PurchasePhase::AwaitingPayment),
            succeeded(PurchasePhase::Delivering),
        ]
    );
}

/// Resumed already settled, an order is returned as it is: no phase is
/// shown, and no call is made that a transient failure could turn into an
/// error.
#[tokio::test]
async fn wait_on_a_settled_order_returns_it_without_a_call() {
    for status in [
        CardOrderStatus::Delivered,
        CardOrderStatus::Refunded,
        CardOrderStatus::RefundOwed,
        CardOrderStatus::NeedsReview,
    ] {
        let settled = initial(status).await;
        // No poll is scripted: any call would panic the fake.
        let fake = FakeBackend::default();
        let (result, phases) = wait(&fake, Some(settled.clone()), &fast()).await;
        assert_eq!(result.unwrap(), settled);
        assert!(phases.is_empty(), "{phases:?}");
        assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 0);
    }
}

#[tokio::test]
async fn an_unpaid_order_past_its_deadline_is_read_as_expired() {
    let overdue = {
        let fake = FakeBackend::default().polls(vec![Ok(Some(past_deadline()))]);
        get_order(&fake, &gateway(), "co-1").await.unwrap()
    };
    assert_eq!(overdue.status, OrderStatus::AwaitingPayment);
    let expired = overdue.at(now_secs());
    assert_eq!(expired.status, OrderStatus::Expired);
    assert!(expired.status.is_terminal());
    assert_eq!(expired.checkout_url, None);

    // One still payable, and one paid, are left as they are.
    let payable = initial(CardOrderStatus::AwaitingPayment).await;
    assert_eq!(payable.clone().at(now_secs()), payable);
    let paid = initial(CardOrderStatus::Processing).await;
    assert_eq!(paid.clone().at(now_secs()), paid);
}

fn past_deadline() -> OrderStatusView {
    OrderStatusView {
        expires_at: Some(1_000_000_000),
        ..view("co-1", CardOrderStatus::AwaitingPayment)
    }
}

#[tokio::test]
async fn wait_reads_an_unpaid_order_past_its_deadline_as_expired() {
    // The backend still reports it, as it does until it prunes it.
    let fake = FakeBackend::default().polls(vec![Ok(Some(past_deadline()))]);
    let (result, phases) = wait(&fake, None, &fast()).await;
    let order = result.unwrap();
    assert_eq!(order.status, OrderStatus::Expired);
    assert_eq!(order.checkout_url, None);
    assert_eq!(
        phases,
        vec![failed(PurchasePhase::AwaitingPayment, "order expired")]
    );

    // Pruned since the last poll: the same answer.
    let fake = FakeBackend::default().polls(vec![Ok(Some(past_deadline())), Ok(None)]);
    let options = WaitOptions {
        // The deadline has passed, but not by the grace: the first poll
        // still reads as payable, and the second finds it gone.
        expiry_grace: Duration::from_secs(u64::MAX / 4),
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    assert_eq!(result.unwrap().status, OrderStatus::Expired);
}

#[tokio::test]
async fn wait_times_out_on_an_unpaid_order() {
    let fake = FakeBackend::default().polls(vec![Ok(Some(view(
        "co-1",
        CardOrderStatus::AwaitingPayment,
    )))]);
    let options = WaitOptions {
        max_unpaid_wait: Duration::ZERO,
        ..fast()
    };
    let (result, phases) = wait(&fake, None, &options).await;
    match result.unwrap_err() {
        CyclesPurchaseError::WaitTimedOut { order } => {
            assert_eq!(order.status, OrderStatus::AwaitingPayment);
            assert!(order.checkout_url.is_some());
        }
        other => panic!("unexpected {other:?}"),
    }
    // The leg was closed out before giving up.
    assert_eq!(
        phases,
        vec![failed(PurchasePhase::AwaitingPayment, "gave up waiting")]
    );
}

#[tokio::test]
async fn wait_times_out_on_a_stuck_paid_order() {
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
    ]);
    let options = WaitOptions {
        max_paid_wait: Duration::ZERO,
        // An unpaid wait that would have ended first must not be the reason.
        max_unpaid_wait: Duration::ZERO,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    match result.unwrap_err() {
        CyclesPurchaseError::WaitTimedOut { order } => {
            assert_eq!(order.status, OrderStatus::Processing)
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn wait_interrupted_returns_last_order() {
    // The interrupt is ready from the start, so whether it or the first poll
    // wins the race, the order handed back is the last one known.
    let fake =
        FakeBackend::default().polls(vec![Ok(Some(view("co-1", CardOrderStatus::Processing)))]);
    let paid = initial(CardOrderStatus::Processing).await;
    let result = wait_for_order(
        &fake,
        &gateway(),
        "co-1",
        Some(paid),
        &WaitOptions::default(),
        ready(()),
        &Reporter::null(),
    )
    .await;
    match result.unwrap_err() {
        CyclesPurchaseError::Interrupted { order } => {
            assert_eq!(order.id, "co-1");
            assert_eq!(order.status, OrderStatus::Processing);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn wait_interrupt_lands_during_a_stalled_poll() {
    // The backend never answers; the interrupt must still get through, with
    // the phase closed out so a renderer does not spin on.
    let fake = FakeBackend {
        stall_polls: true,
        ..FakeBackend::default()
    };
    let (reporter, mut events) = icp_events::channel();
    let result = wait_for_order(
        &fake,
        &gateway(),
        "co-1",
        None,
        &WaitOptions::default(),
        ready(()),
        &reporter,
    )
    .await;
    drop(reporter);
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::Interrupted { .. }
    ));
    let mut completed = 0;
    while let Some(event) = events.recv().await {
        // Stopped, not failed: the order carries on without the command.
        if let EventKind::TaskCompleted {
            outcome: TaskOutcome::Stopped { message },
        } = event.kind
        {
            assert_eq!(message, "stopped waiting");
            completed += 1;
        }
    }
    assert_eq!(completed, 1);
}

#[test]
fn statuses_tell_paid_and_settled_apart() {
    for status in [
        OrderStatus::AwaitingPayment,
        OrderStatus::PaymentPending,
        OrderStatus::Processing,
    ] {
        assert!(!status.is_terminal(), "{status}");
    }
    for status in [
        OrderStatus::Delivered,
        OrderStatus::NeedsReview,
        OrderStatus::Refunding,
        OrderStatus::Refunded,
        OrderStatus::RefundOwed,
        OrderStatus::Expired,
    ] {
        assert!(status.is_terminal(), "{status}");
    }
    assert!(!OrderStatus::AwaitingPayment.is_paid());
    assert!(!OrderStatus::Expired.is_paid());
    assert!(OrderStatus::PaymentPending.is_paid());
    assert!(OrderStatus::RefundOwed.is_paid());
    assert_eq!(OrderStatus::RefundOwed.tag(), "refund_owed");
    assert_eq!(OrderStatus::RefundOwed.to_string(), "owed a refund");
}
