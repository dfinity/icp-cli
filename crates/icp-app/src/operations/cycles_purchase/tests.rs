use std::collections::VecDeque;
use std::future::{pending, ready};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use candid::{Decode, Encode, Int, Nat, Principal};
use icp_canister_interfaces::cycles_gateway::{
    Account, Amount, CANCEL_ORDER_METHOD, CREATE_ORDER_METHOD, CancelOrderError, CancelOrderResult,
    CreateOrderError, CreateOrderResult, CreatedOrder, Destination, GET_ORDER_METHOD,
    LIST_ORDERS_METHOD, ListOrdersResult, Order, OrderStatus, QUOTE_PREVIEWS_METHOD, QuotePreview,
    QuotePreviews, Reason,
};
use icp_project::calls::{Authority, Call, CallError, CanisterCalls};

use super::*;

const GATEWAY: &str = "saz2a-riaaa-aaaay-aadha-cai";
const BUYER: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

fn principal(text: &str) -> Principal {
    Principal::from_text(text).unwrap()
}

fn gateway() -> Principal {
    principal(GATEWAY)
}

fn usd(cents: u64) -> Money {
    Money::from_minor_units(cents, Currency::Usd)
}

fn order(id: &str, status: OrderStatus) -> Order {
    Order {
        id: id.to_owned(),
        status,
        locked_cycles: Nat::from(7_238_000_000_000u64),
        stripe_session_url: Some(format!("https://checkout.example/{id}")),
        // Far enough in the future that no test trips the expiry grace.
        expires_at_ns: Some(Int::from(i64::MAX / 2)),
        paid_usd_cents: None,
        expired_by: None,
        abandoned_reason: None,
        destination: Destination::CyclesLedgerAccount(Account {
            owner: principal(BUYER),
            subaccount: None,
        }),
    }
}

fn created(order: Order) -> CreateOrderResult {
    CreateOrderResult::Ok(CreatedOrder { order })
}

fn refused(error: CreateOrderError) -> CreateOrderResult {
    CreateOrderResult::Err(error)
}

fn unanswered() -> CallError {
    CallError::unanswered(
        gateway(),
        GET_ORDER_METHOD,
        std::io::Error::other("connection reset"),
    )
}

fn rejected(message: &str) -> CallError {
    CallError::Rejected {
        canister: gateway(),
        method: GET_ORDER_METHOD.to_owned(),
        code: Some("IC0503".to_owned()),
        message: message.to_owned(),
    }
}

/// A cycles gateway whose replies are scripted per method.
#[derive(Default)]
struct FakeGateway {
    /// The one preview `quote_previews` answers with; `None` answers with
    /// no previews at all.
    quote: Option<QuotePreview>,
    /// One reply per `create_order` call, in order.
    create: Mutex<VecDeque<CreateOrderResult>>,
    /// One reply per `get_order` poll, in order. An `Err` is the network
    /// failing to answer, or rejecting.
    get_order: Mutex<VecDeque<Result<Option<Order>, CallError>>>,
    /// Pages of `list_orders`, in cursor order.
    list_orders: Vec<Vec<Order>>,
    cancel: Option<CancelOrderResult>,
    /// Every call made, as (method, candid argument).
    calls_seen: Mutex<Vec<(String, Vec<u8>)>>,
    /// Rejects every call as if the canister did not exist.
    absent: bool,
}

impl FakeGateway {
    fn with_quote(cents: u64, fee_cents: u64, cycles: Option<u128>) -> Self {
        Self {
            quote: Some(QuotePreview {
                cycles: cycles.map(Nat::from),
                fee_cents: Nat::from(fee_cents),
                net_cents: Some(Nat::from(cents - fee_cents)),
                usd_cents: Nat::from(cents),
            }),
            ..Self::default()
        }
    }

    fn polls(self, replies: Vec<Result<Option<Order>, CallError>>) -> Self {
        *self.get_order.lock().unwrap() = replies.into();
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
        assert_eq!(call.canister, gateway());
        self.calls_seen
            .lock()
            .unwrap()
            .push((call.method.clone(), call.arg.clone()));
        if self.absent {
            return Err(CallError::Rejected {
                canister: call.canister,
                method: call.method.clone(),
                code: Some("IC0301".to_owned()),
                message: format!("Canister {GATEWAY} not found"),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl CanisterCalls for FakeGateway {
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
            CANCEL_ORDER_METHOD => {
                Ok(Encode!(self.cancel.as_ref().expect("no cancel reply")).unwrap())
            }
            other => panic!("unexpected update {other}"),
        }
    }

    async fn query(&self, call: Call) -> Result<Vec<u8>, CallError> {
        self.record(&call)?;
        match call.method.as_str() {
            QUOTE_PREVIEWS_METHOD => {
                let previews = QuotePreviews {
                    quotes: self.quote.iter().cloned().collect(),
                };
                Ok(Encode!(&previews).unwrap())
            }
            GET_ORDER_METHOD => {
                let reply = self
                    .get_order
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("ran out of scripted get_order replies")?;
                Ok(Encode!(&reply).unwrap())
            }
            LIST_ORDERS_METHOD => {
                let (cursor, _limit) = Decode!(&call.arg, Option<String>, Nat).unwrap();
                let index: usize = cursor.map(|c| c.parse().unwrap()).unwrap_or(0);
                let page = ListOrdersResult {
                    next_cursor: (index + 1 < self.list_orders.len())
                        .then(|| (index + 1).to_string()),
                    orders: self.list_orders.get(index).cloned().unwrap_or_default(),
                };
                Ok(Encode!(&page).unwrap())
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

fn ten_dollars() -> Quote {
    Quote {
        amount: usd(1000),
        fee: usd(59),
        cycles: 7_238_000_000_000,
    }
}

// --- quote -----------------------------------------------------------------

#[tokio::test]
async fn quote_maps_preview_to_quote() {
    let fake = FakeGateway::with_quote(1000, 59, Some(7_238_000_000_000));
    let q = quote(&fake, gateway(), usd(1000)).await.unwrap();
    assert_eq!(q, ten_dollars());
    assert_eq!(q.minimum_cycles(), 7_238_000_000_000 * 95 / 100);
    let amounts = Decode!(&fake.arg_of(QUOTE_PREVIEWS_METHOD), Vec<Nat>).unwrap();
    assert_eq!(amounts, vec![Nat::from(1000u32)]);
}

#[tokio::test]
async fn quote_without_rate_is_rate_unavailable() {
    let fake = FakeGateway::with_quote(1000, 59, None);
    let err = quote(&fake, gateway(), usd(1000)).await.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::RateUnavailable { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn quote_without_preview_is_missing() {
    let fake = FakeGateway::default();
    let err = quote(&fake, gateway(), usd(1000)).await.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::QuoteMissing { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn quote_on_missing_gateway_is_gateway_not_found() {
    let fake = FakeGateway {
        absent: true,
        ..FakeGateway::default()
    };
    let err = quote(&fake, gateway(), usd(1000)).await.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );
}

// --- place_order -----------------------------------------------------------

#[tokio::test]
async fn place_order_sends_caller_destination_and_min_cycles() {
    let fake = FakeGateway::default().creates(vec![created(order("o1", OrderStatus::Created))]);
    let view = place_order(&fake, gateway(), &ten_dollars()).await.unwrap();
    assert_eq!(view.id, "o1");
    assert_eq!(view.status, OrderStatus::Created);
    assert_eq!(view.locked_cycles, 7_238_000_000_000);
    assert_eq!(
        view.checkout_url.as_deref(),
        Some("https://checkout.example/o1")
    );

    let (amount, destination, min) = Decode!(
        &fake.arg_of(CREATE_ORDER_METHOD),
        Amount,
        Destination,
        Option<Nat>
    )
    .unwrap();
    assert_eq!(amount, Amount::Custom(Nat::from(1000u32)));
    assert_eq!(
        destination,
        Destination::CyclesLedgerAccount(Account {
            owner: principal(BUYER),
            subaccount: None,
        })
    );
    assert_eq!(min, Some(Nat::from(7_238_000_000_000u128 * 95 / 100)));
}

#[tokio::test]
async fn place_order_on_missing_gateway_is_gateway_not_found() {
    let fake = FakeGateway {
        absent: true,
        ..FakeGateway::default()
    };
    let err = place_order(&fake, gateway(), &ten_dollars())
        .await
        .unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn too_many_open_orders_resolves_via_list_orders() {
    let too_many = refused(CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders {
        max: Nat::from(1u8),
        open: Nat::from(1u8),
    }));
    let fake = FakeGateway {
        list_orders: vec![vec![
            order("old", OrderStatus::Cancelled),
            order("open", OrderStatus::Paid),
        ]],
        ..FakeGateway::default()
    }
    .creates(vec![too_many]);

    let err = place_order(&fake, gateway(), &ten_dollars())
        .await
        .unwrap_err();
    match err {
        CyclesPurchaseError::OpenOrderExists { order, open, max } => {
            assert_eq!(order.id, "open");
            assert_eq!(order.status, OrderStatus::Paid);
            assert_eq!((open, max), (1, 1));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn too_many_open_orders_without_a_visible_one_is_a_refusal() {
    let too_many = refused(CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders {
        max: Nat::from(1u8),
        open: Nat::from(1u8),
    }));
    let fake = FakeGateway {
        list_orders: vec![vec![order("old", OrderStatus::Delivered)]],
        ..FakeGateway::default()
    }
    .creates(vec![too_many]);

    let err = place_order(&fake, gateway(), &ten_dollars())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            CyclesPurchaseError::OrderRefused {
                error: CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders { .. })
            }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn quote_changed_is_surfaced_as_refusal() {
    let fake = FakeGateway::default().creates(vec![refused(CreateOrderError::QuoteChanged {
        minimum: Nat::from(10u8),
        quoted: Nat::from(9u8),
    })]);
    let err = place_order(&fake, gateway(), &ten_dollars())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            CyclesPurchaseError::OrderRefused {
                error: CreateOrderError::QuoteChanged { .. }
            }
        ),
        "{err}"
    );
}

// --- get_order / find_open_order / cancel_order ----------------------------

#[tokio::test]
async fn get_order_none_is_order_not_found() {
    let fake = FakeGateway::default().polls(vec![Ok(None)]);
    let err = get_order(&fake, gateway(), "ghost").await.unwrap_err();
    match err {
        CyclesPurchaseError::OrderNotFound { id, caller, .. } => {
            assert_eq!(id, "ghost");
            assert_eq!(caller, principal(BUYER));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn find_open_order_follows_cursor() {
    let fake = FakeGateway {
        list_orders: vec![
            vec![order("a", OrderStatus::Delivered)],
            vec![
                order("b", OrderStatus::Expired),
                order("c", OrderStatus::Created),
            ],
        ],
        ..FakeGateway::default()
    };
    let found = find_open_order(&fake, gateway()).await.unwrap().unwrap();
    assert_eq!(found.id, "c");
    assert_eq!(fake.count_of(LIST_ORDERS_METHOD), 2);
}

#[tokio::test]
async fn find_open_order_none_when_all_settled() {
    let fake = FakeGateway {
        list_orders: vec![vec![order("a", OrderStatus::Delivered)]],
        ..FakeGateway::default()
    };
    assert!(find_open_order(&fake, gateway()).await.unwrap().is_none());
}

#[tokio::test]
async fn cancel_order_returns_the_order() {
    let fake = FakeGateway {
        cancel: Some(CancelOrderResult::Ok(Box::new(order(
            "o1",
            OrderStatus::Cancelled,
        )))),
        ..FakeGateway::default()
    };
    let view = cancel_order(&fake, gateway(), "o1").await.unwrap();
    assert_eq!(view.status, OrderStatus::Cancelled);
    let id = Decode!(&fake.arg_of(CANCEL_ORDER_METHOD), String).unwrap();
    assert_eq!(id, "o1");
}

#[tokio::test]
async fn cancel_order_maps_refusal() {
    let fake = FakeGateway {
        cancel: Some(CancelOrderResult::Err(CancelOrderError::NotCancellable {
            status: OrderStatus::Delivered,
        })),
        ..FakeGateway::default()
    };
    let err = cancel_order(&fake, gateway(), "o1").await.unwrap_err();
    assert!(
        matches!(
            err,
            CyclesPurchaseError::CancelRefused {
                error: CancelOrderError::NotCancellable {
                    status: OrderStatus::Delivered
                },
                ..
            }
        ),
        "{err}"
    );
}

// --- wait_for_order --------------------------------------------------------

async fn wait(
    fake: &FakeGateway,
    initial: Option<OrderView>,
    options: &WaitOptions,
) -> (Result<OrderView, CyclesPurchaseError>, Vec<OrderStatus>) {
    let mut seen = Vec::new();
    let result = wait_for_order(
        fake,
        gateway(),
        "o1",
        initial,
        options,
        pending(),
        &mut |o| seen.push(o.status),
    )
    .await;
    (result, seen)
}

#[tokio::test]
async fn wait_happy_path_created_paid_delivered() {
    let fake = FakeGateway::default().polls(vec![
        Ok(Some(order("o1", OrderStatus::Created))),
        Ok(Some(order("o1", OrderStatus::Created))),
        Ok(Some(order("o1", OrderStatus::Paid))),
        Ok(Some(order("o1", OrderStatus::Delivered))),
    ]);
    let (result, seen) = wait(&fake, None, &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
    assert_eq!(
        seen,
        vec![
            OrderStatus::Created,
            OrderStatus::Paid,
            OrderStatus::Delivered
        ]
    );
    assert_eq!(fake.count_of(GET_ORDER_METHOD), 4);
}

#[tokio::test]
async fn wait_does_not_report_the_initial_status_again() {
    let fake = FakeGateway::default().polls(vec![
        Ok(Some(order("o1", OrderStatus::Created))),
        Ok(Some(order("o1", OrderStatus::Delivered))),
    ]);
    let initial: OrderView = order("o1", OrderStatus::Created).try_into().unwrap();
    let (result, seen) = wait(&fake, Some(initial), &fast()).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
    assert_eq!(seen, vec![OrderStatus::Delivered]);
}

#[tokio::test]
async fn wait_returns_every_terminal_status_as_ok() {
    for status in [
        OrderStatus::Expired,
        OrderStatus::Cancelled,
        OrderStatus::Abandoned,
        OrderStatus::NeedsReview,
    ] {
        let fake = FakeGateway::default().polls(vec![Ok(Some(order("o1", status)))]);
        let (result, seen) = wait(&fake, None, &fast()).await;
        assert_eq!(result.unwrap().status, status);
        assert_eq!(seen, vec![status]);
    }
}

#[tokio::test]
async fn wait_tolerates_transient_errors() {
    let fake = FakeGateway::default().polls(vec![
        Err(unanswered()),
        Err(unanswered()),
        Ok(Some(order("o1", OrderStatus::Created))),
        Err(unanswered()),
        Err(unanswered()),
        Ok(Some(order("o1", OrderStatus::Delivered))),
    ]);
    let options = WaitOptions {
        max_consecutive_transient: 2,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    assert_eq!(result.unwrap().status, OrderStatus::Delivered);
}

#[tokio::test]
async fn wait_gives_up_after_consecutive_transient_errors() {
    let fake = FakeGateway::default().polls(vec![
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
    assert_eq!(fake.count_of(GET_ORDER_METHOD), 3);
}

#[tokio::test]
async fn wait_fails_fast_on_non_transient_error() {
    let fake = FakeGateway::default().polls(vec![Err(rejected("canister trapped"))]);
    let (result, _) = wait(&fake, None, &fast()).await;
    let err = result.unwrap_err();
    assert!(matches!(err, CyclesPurchaseError::GetOrder { .. }), "{err}");
    assert_eq!(fake.count_of(GET_ORDER_METHOD), 1);
}

#[tokio::test]
async fn wait_not_found_is_order_not_found() {
    let fake = FakeGateway::default().polls(vec![Ok(None)]);
    let (result, _) = wait(&fake, None, &fast()).await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::OrderNotFound { .. }
    ));
}

#[tokio::test]
async fn wait_times_out_after_expiry_grace() {
    let mut expired_long_ago = order("o1", OrderStatus::Created);
    expired_long_ago.expires_at_ns = Some(Int::from(1_000_000_000i64));
    let fake = FakeGateway::default().polls(vec![Ok(Some(expired_long_ago))]);
    let options = WaitOptions {
        expiry_grace: Duration::ZERO,
        ..fast()
    };
    let (result, seen) = wait(&fake, None, &options).await;
    match result.unwrap_err() {
        CyclesPurchaseError::WaitTimedOut { order } => {
            assert_eq!(order.status, OrderStatus::Created);
        }
        other => panic!("unexpected {other:?}"),
    }
    // The status was still reported before giving up.
    assert_eq!(seen, vec![OrderStatus::Created]);
}

#[tokio::test]
async fn wait_times_out_without_expiry() {
    let mut no_deadline = order("o1", OrderStatus::Created);
    no_deadline.expires_at_ns = None;
    let fake = FakeGateway::default().polls(vec![Ok(Some(no_deadline))]);
    let options = WaitOptions {
        max_without_expiry: Duration::ZERO,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::WaitTimedOut { .. }
    ));
}

#[tokio::test]
async fn wait_times_out_on_a_stuck_paid_order() {
    let fake = FakeGateway::default().polls(vec![
        Ok(Some(order("o1", OrderStatus::Paid))),
        Ok(Some(order("o1", OrderStatus::Paid))),
    ]);
    let options = WaitOptions {
        max_paid_wait: Duration::ZERO,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    match result.unwrap_err() {
        CyclesPurchaseError::WaitTimedOut { order } => assert_eq!(order.status, OrderStatus::Paid),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn wait_interrupted_returns_last_order() {
    let fake = FakeGateway::default().polls(vec![Ok(Some(order("o1", OrderStatus::Paid)))]);
    let result = wait_for_order(
        &fake,
        gateway(),
        "o1",
        None,
        &WaitOptions::default(),
        ready(()),
        &mut |_| {},
    )
    .await;
    match result.unwrap_err() {
        CyclesPurchaseError::Interrupted { order } => {
            assert_eq!(order.id, "o1");
            assert_eq!(order.status, OrderStatus::Paid);
        }
        other => panic!("unexpected {other:?}"),
    }
}
