use std::collections::VecDeque;
use std::future::{pending, ready};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use candid::{Decode, Encode, Nat, Principal};
use icp_project::calls::{Authority, Call, CallError, CanisterCalls};

use super::interface::{
    CARD_TERMS_METHOD, CREATE_ORDER_METHOD, CardOrderStatus, CardTerms, CreateOrderArgs,
    CreateOrderResult, CreatedOrder, Destination, ORDER_STATUS_METHOD, OrderDestination,
    OrderError, OrderStatusView, XDR_RATE_STATUS_METHOD, XdrRateStatus,
};
use super::money::{Currency, Money};
use super::pricing::{Terms, ledger_cycles};
use super::purchase::*;

const BACKEND: &str = "2bwus-iiaaa-aaaac-bee3a-cai";
const BUYER: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

/// The Payment Link the fake backend publishes.
const PAYMENT_LINK: &str = "https://buy.stripe.com/test_link";

/// $1.3520 an XDR.
const RATE: u64 = 13_520;

fn principal(text: &str) -> Principal {
    Principal::from_text(text).unwrap()
}

/// CyclePay's published terms: $1 to $200, Stripe's 2.9% + $0.30, two Cycles
/// Ledger fees on a delivery.
fn card_terms() -> CardTerms {
    CardTerms {
        deliveries_paused: false,
        min_order_usd_cents: 100,
        max_order_usd_cents: 20_000,
        fee_bps: 290,
        fee_fixed_usd_cents: 30,
        ledger_delivery_fee_cycles: Nat::from(200_000_000u64),
    }
}

/// The gateway as [`connect`] builds it on [`card_terms`].
fn gateway() -> Gateway {
    Gateway::new(principal(BACKEND), card_terms()).unwrap()
}

fn terms() -> Terms {
    gateway().terms
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
        amount_usd_cents: cents,
        net_usd_cents: super::pricing::net_usd_cents(cents, &terms()),
        payment_url: format!("{PAYMENT_LINK}?client_reference_id=o_{id}&prefilled_amount={cents}"),
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
    /// What `get_card_terms` answers; `None` answers [`card_terms`].
    terms: Option<CardTerms>,
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
    /// The next call fails with this, as the network reported it.
    refusal: Mutex<Option<CallError>>,
    /// Never answers `order_status`, as a backend behind a stalled connection.
    stall_polls: bool,
    /// Never answers `create_order`, as a backend behind a stalled
    /// connection.
    stall_creates: bool,
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
        if let Some(err) = self.refusal.lock().unwrap().take() {
            return Err(err);
        }
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
                if self.stall_creates {
                    pending::<()>().await;
                }
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
            CARD_TERMS_METHOD => {
                let terms = self.terms.clone().unwrap_or_else(card_terms);
                Ok(Encode!(&terms).unwrap())
            }
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

// --- connect ---------------------------------------------------------------

#[tokio::test]
async fn connect_reads_the_terms_the_gateway_publishes() {
    let fake = FakeBackend::default();
    let gateway = connect(&fake, principal(BACKEND)).await.unwrap();
    assert_eq!(gateway.canister, principal(BACKEND));
    assert!(!gateway.paused);
    assert_eq!(
        gateway.terms,
        Terms {
            min_order_usd_cents: 100,
            max_order_usd_cents: 20_000,
            fee_bps: 290,
            fee_fixed_usd_cents: 30,
            ledger_delivery_fee_cycles: 200_000_000,
        }
    );
    assert_eq!(fake.count_of(CARD_TERMS_METHOD), 1);
}

#[tokio::test]
async fn connect_on_missing_gateway_is_gateway_not_found() {
    let fake = FakeBackend {
        absent: true,
        ..FakeBackend::default()
    };
    let err = connect(&fake, principal(BACKEND)).await.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );
    // Which network would have it is not said here: CyclePay's own gateway
    // is refused off mainnet before it is asked anything, and any other
    // gateway could be anywhere.
    assert_eq!(
        err.to_string(),
        format!("the cycles gateway canister {BACKEND} does not exist on this network")
    );
}

/// A boundary node, or a local HTTP gateway, refuses to route to a canister
/// it has never heard of before any replica sees the call: an HTTP 400 with
/// no reject code, which the call layer reports as a failure. The command
/// reads it as the gateway being absent, as it would a replica's IC0301. Any
/// other HTTP failure stays what it is.
#[tokio::test]
async fn connect_reads_an_unroutable_canister_as_gateway_not_found() {
    let http = |status: u16, body: &str| {
        CallError::failed(
            principal(BACKEND),
            CARD_TERMS_METHOD,
            ic_agent::AgentError::HttpError(ic_agent::agent::agent_error::HttpErrorPayload {
                status,
                content_type: Some("text/plain; charset=utf-8".to_owned()),
                content: body.as_bytes().to_vec(),
            }),
        )
    };
    let fake = FakeBackend::default();
    *fake.refusal.lock().unwrap() = Some(http(
        400,
        "error: canister_not_found\ndetails: The specified canister does not exist.",
    ));
    let err = connect(&fake, principal(BACKEND)).await.unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );

    // Only the boundary node's own words say so: another 4xx, or a 5xx,
    // stays a failure to read the terms.
    for (status, body) in [
        (
            400,
            "error: invalid_request\ndetails: the request could not be parsed",
        ),
        (403, "forbidden"),
        (502, "bad gateway"),
    ] {
        let fake = FakeBackend::default();
        *fake.refusal.lock().unwrap() = Some(http(status, body));
        let err = connect(&fake, principal(BACKEND)).await.unwrap_err();
        assert!(
            matches!(err, CyclesPurchaseError::ReadTerms { .. }),
            "{status}: {err}"
        );
    }
}

/// A canister that is there but serves no `get_card_terms` is not a gateway,
/// and is said to be none, rather than in the replica's words.
#[tokio::test]
async fn connect_on_a_canister_that_is_no_gateway_says_so() {
    for (code, message) in [
        (
            Some("IC0536"),
            "Canister ryjl3 has no query method 'get_card_terms'",
        ),
        (None, "Canister ryjl3 has no query method 'get_card_terms'"),
    ] {
        let fake = FakeBackend::default();
        *fake.refusal.lock().unwrap() = Some(CallError::Rejected {
            canister: principal(BACKEND),
            method: CARD_TERMS_METHOD.to_owned(),
            code: code.map(str::to_owned),
            message: message.to_owned(),
        });
        let err = connect(&fake, principal(BACKEND)).await.unwrap_err();
        assert!(
            matches!(err, CyclesPurchaseError::NotAGateway { .. }),
            "{err}"
        );
        assert_eq!(
            err.to_string(),
            format!(
                "canister {BACKEND} is not a cycles gateway: it does not serve \
                 get_card_terms, the method a gateway publishes its terms with"
            )
        );
    }
}

/// A principal no canister can have is refused before anything is asked.
#[test]
fn a_gateway_must_be_a_canister_id() {
    assert!(check_gateway_id(principal(BACKEND)).is_ok());
    for (gateway, what) in [
        ("aaaaa-aa", "the management canister"),
        ("2vxsx-fae", "the anonymous principal"),
        (
            "7k7px-dcypq-wulnh-byzgd-6sib2-udn6c-lfo6e-6oujd-veifp-62woo-5ae",
            "not a canister id",
        ),
    ] {
        let err = check_gateway_id(principal(gateway)).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("{gateway} cannot be a cycles gateway: it is {what}")
        );
    }
}

/// Terms the CLI could not quote on are refused at once, before any quote.
#[tokio::test]
async fn connect_refuses_terms_it_cannot_act_on() {
    let fake = FakeBackend {
        terms: Some(CardTerms {
            min_order_usd_cents: 200,
            max_order_usd_cents: 100,
            ..card_terms()
        }),
        ..FakeBackend::default()
    };
    let err = connect(&fake, principal(BACKEND)).await.unwrap_err();
    assert!(
        matches!(&err, CyclesPurchaseError::InvalidTerms { .. })
            && err.to_string().contains("allows no order"),
        "{err}"
    );
    let fake = FakeBackend {
        terms: Some(CardTerms {
            ledger_delivery_fee_cycles: Nat::from(u128::MAX) + Nat::from(1u8),
            ..card_terms()
        }),
        ..FakeBackend::default()
    };
    let err = connect(&fake, principal(BACKEND)).await.unwrap_err();
    assert!(err.to_string().contains("ledger delivery fee"), "{err}");
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
    assert_eq!(q.cycles, ledger_cycles(2500, RATE, &terms()));
    assert_eq!(q.rate.xdr_usd_x10000, RATE);
    assert!(!q.rate.stale);
    assert_eq!(q.rate.age, Some(Duration::from_secs(120)));
    assert_eq!(q.rate.usd_per_trillion(), "1.3520 USD");
    assert_eq!(fake.count_of(XDR_RATE_STATUS_METHOD), 1);
}

#[tokio::test]
async fn quote_outside_the_range_is_refused_before_any_order() {
    let fake = FakeBackend::priced();
    for cents in [100 - 1, 20_000 + 1] {
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
            &gateway(),
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

/// The range, the fee and the delivery fee are the gateway's: a quote on
/// another gateway's terms is made on them, with nothing of CyclePay's
/// left in.
#[tokio::test]
async fn quote_is_made_on_the_gateways_own_terms() {
    let fake = FakeBackend {
        rate: Some(rate()),
        terms: Some(CardTerms {
            min_order_usd_cents: 500,
            max_order_usd_cents: 50_000,
            fee_bps: 0,
            fee_fixed_usd_cents: 0,
            ledger_delivery_fee_cycles: Nat::from(0u8),
            ..card_terms()
        }),
        ..FakeBackend::default()
    };
    let gateway = connect(&fake, principal(BACKEND)).await.unwrap();
    // $1 is under this gateway's minimum, and $300 within its maximum.
    let err = quote_target(&fake, &gateway, Target::Spend(usd(100)))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CyclesPurchaseError::AmountOutOfRange { min, max, .. }
            if *min == usd(500) && *max == usd(50_000)),
        "{err}"
    );
    let q = quote_target(&fake, &gateway, Target::Spend(usd(30_000)))
        .await
        .unwrap();
    assert_eq!(q.fee, usd(0));
    assert_eq!(q.net, usd(30_000));
    assert_eq!(q.cycles, super::pricing::cycles_for_net(30_000, RATE));
    // The least that buys a sum is found within this range.
    let q = quote_target(&fake, &gateway, receive(1)).await.unwrap();
    assert_eq!(q.amount, usd(500));
}

/// A paused gateway refuses every quote, so no order is tried on it.
#[tokio::test]
async fn quote_is_refused_while_the_gateway_is_paused() {
    let fake = FakeBackend {
        rate: Some(rate()),
        terms: Some(CardTerms {
            deliveries_paused: true,
            ..card_terms()
        }),
        ..FakeBackend::default()
    };
    let gateway = connect(&fake, principal(BACKEND)).await.unwrap();
    assert!(gateway.paused);
    let err = quote_target(&fake, &gateway, Target::Spend(usd(1000)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::CardsPaused { .. }),
        "{err}"
    );
    assert!(err.to_string().contains("not taking card orders"), "{err}");
    assert_eq!(fake.count_of(CREATE_ORDER_METHOD), 0);
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
    assert!(ledger_cycles(q.amount.minor_units - 1, RATE, &terms()) < target);
    assert_eq!(
        q.cycles,
        ledger_cycles(q.amount.minor_units, RATE, &terms())
    );
    assert_eq!(q.fee.minor_units + q.net.minor_units, q.amount.minor_units);
}

#[tokio::test]
async fn quote_receive_below_the_minimum_pays_the_minimum() {
    let fake = FakeBackend::priced();
    let q = quote_target(&fake, &gateway(), receive(1)).await.unwrap();
    assert_eq!(q.amount, usd(100));
    assert_eq!(q.cycles, ledger_cycles(100, RATE, &terms()));
}

#[tokio::test]
async fn quote_receive_beyond_the_maximum_names_what_it_buys() {
    let fake = FakeBackend::priced();
    let most = ledger_cycles(20_000, RATE, &terms());
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
    let order = place_order(&fake, &gateway(), usd(2500), pending())
        .await
        .unwrap();
    assert_eq!(order.id, "co-1");
    assert_eq!(order.status, Some(CardOrderStatus::AwaitingPayment));
    assert_eq!(order.amount, usd(2500));
    assert_eq!(order.net, usd(2398));
    assert_eq!(order.expires_at, Some(FAR_FUTURE));
    assert_eq!(
        order.checkout_url.as_deref(),
        Some(format!("{PAYMENT_LINK}?client_reference_id=o_co-1&prefilled_amount=2500").as_str())
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

/// The page to pay on is the gateway's to hand out: it is opened as it is,
/// whatever the CLI would have built from the link and the order id.
#[tokio::test]
async fn place_order_hands_out_the_gateways_payment_page_as_it_is() {
    let CreateOrderResult::Ok(mut order) = created("co-1", 2500) else {
        unreachable!()
    };
    order.payment_url = "https://pay.example/checkout/abc?ref=xyz".to_owned();
    let fake = FakeBackend::default().creates(vec![CreateOrderResult::Ok(order)]);
    let order = place_order(&fake, &gateway(), usd(2500), pending())
        .await
        .unwrap();
    assert_eq!(
        order.checkout_url.as_deref(),
        Some("https://pay.example/checkout/abc?ref=xyz")
    );
}

/// A card is typed on the page, so anything but an https URL is refused.
#[tokio::test]
async fn place_order_refuses_a_payment_page_that_is_not_https() {
    for bad in ["http://pay.example/abc", "javascript:alert(1)", "not a url"] {
        let CreateOrderResult::Ok(mut order) = created("co-1", 2500) else {
            unreachable!()
        };
        order.payment_url = bad.to_owned();
        let fake = FakeBackend::default().creates(vec![CreateOrderResult::Ok(order)]);
        let err = place_order(&fake, &gateway(), usd(2500), pending())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, CyclesPurchaseError::InvalidPaymentUrl { id, url, .. }
                if id == "co-1" && url == bad),
            "{bad}: {err}"
        );
    }
}

#[tokio::test]
async fn place_order_maps_a_refusal() {
    let fake = FakeBackend::default().creates(vec![refused("RATE_LIMITED")]);
    let err = place_order(&fake, &gateway(), usd(2500), pending())
        .await
        .unwrap_err();
    match err {
        CyclesPurchaseError::OrderRefused { error } => {
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
    let err = place_order(&fake, &gateway(), usd(2500), pending())
        .await
        .unwrap_err();
    assert!(
        matches!(err, CyclesPurchaseError::GatewayNotFound { .. }),
        "{err}"
    );
}

/// A stop while `create_order` is stalled ends the call at once: whether
/// the order was made is then not known, and said not to be.
#[tokio::test]
async fn place_order_gives_way_to_an_interrupt_while_the_call_stalls() {
    let fake = FakeBackend {
        stall_creates: true,
        ..FakeBackend::default()
    };
    let placed = tokio::time::timeout(
        Duration::from_secs(5),
        place_order(&fake, &gateway(), usd(2500), ready(())),
    )
    .await
    .expect("place_order did not give way to the interrupt");
    assert!(matches!(
        placed.unwrap_err(),
        CyclesPurchaseError::InterruptedPlacing { .. }
    ),);
    assert_eq!(fake.count_of(CREATE_ORDER_METHOD), 1);
}

// --- wait_for_order --------------------------------------------------------

/// Run the wait and collect the phases it told of, in order.
async fn wait(
    fake: &FakeBackend,
    initial: Option<OrderView>,
    options: &WaitOptions,
) -> (Result<OrderView, CyclesPurchaseError>, Vec<PurchasePhase>) {
    let mut phases = Vec::new();
    let result = wait_for_order(
        fake,
        &gateway(),
        "co-1",
        initial,
        options,
        pending(),
        |phase| phases.push(phase),
    )
    .await;
    (result, phases)
}

/// An order as the wait last saw it.
fn initial(status: CardOrderStatus) -> OrderView {
    OrderView::read(view("co-1", status))
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
    assert_eq!(order.status, Some(CardOrderStatus::Delivered));
    assert_eq!(order.cycles_delivered, Some(17_736_486_390_532));
    // Pending and processing are one leg: the card has been charged.
    assert_eq!(
        phases,
        vec![PurchasePhase::AwaitingPayment, PurchasePhase::Delivering]
    );
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 5);
}

#[tokio::test]
async fn wait_starts_in_the_initial_orders_phase() {
    let fake = FakeBackend::default().polls(vec![
        Ok(Some(view("co-1", CardOrderStatus::Processing))),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let paid = initial(CardOrderStatus::Processing);
    let (result, phases) = wait(&fake, Some(paid), &fast()).await;
    assert_eq!(result.unwrap().status, Some(CardOrderStatus::Delivered));
    // Already paid: no "awaiting payment" leg is reported.
    assert_eq!(phases, vec![PurchasePhase::Delivering]);
}

/// A refund is read with what was refunded and why.
#[tokio::test]
async fn wait_reads_a_refund() {
    let mut refunded = view("co-1", CardOrderStatus::Refunded);
    refunded.detail = Some("OVER_CAP".into());
    let fake = FakeBackend::default().polls(vec![Ok(Some(refunded))]);
    let (result, _) = wait(&fake, None, &fast()).await;
    let order = result.unwrap();
    assert_eq!(order.status, Some(CardOrderStatus::Refunded));
    assert_eq!(order.refunded, Some(usd(2500)));
    assert_eq!(order.paid, Some(usd(2500)));
    assert_eq!(order.detail.as_deref(), Some("OVER_CAP"));
    assert_eq!(order.checkout_url, None);
    assert_eq!(order.expires_at, None);
    let id = Decode!(&fake.arg_of(ORDER_STATUS_METHOD), String).unwrap();
    assert_eq!(id, "co-1");
}

#[tokio::test]
async fn wait_returns_every_settled_status_as_ok() {
    for status in [
        CardOrderStatus::Refunding,
        CardOrderStatus::Refunded,
        CardOrderStatus::RefundOwed,
        CardOrderStatus::NeedsReview,
    ] {
        let fake = FakeBackend::default().polls(vec![Ok(Some(view("co-1", status)))]);
        let (result, phases) = wait(&fake, None, &fast()).await;
        assert_eq!(result.unwrap().status, Some(status));
        assert_eq!(phases, vec![PurchasePhase::AwaitingPayment]);
    }
}

/// A poll that says nothing of the order is tried again, whatever kept it
/// from answering: the network, the backend refusing the call (as it does
/// for a few minutes during an upgrade), or a replica that has not seen the
/// order yet.
#[tokio::test]
async fn wait_retries_a_poll_that_said_nothing() {
    let fake = FakeBackend::default().polls(vec![
        Err(unanswered()),
        Ok(None),
        Err(rejected("Canister is stopped")),
        Ok(Some(view("co-1", CardOrderStatus::AwaitingPayment))),
        Err(unanswered()),
        Err(unanswered()),
        Err(unanswered()),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let options = WaitOptions {
        max_consecutive_misses: 3,
        ..fast()
    };
    let (result, _) = wait(&fake, None, &options).await;
    assert_eq!(result.unwrap().status, Some(CardOrderStatus::Delivered));
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 8);
}

#[tokio::test]
async fn wait_gives_up_after_consecutive_failed_polls() {
    let fake = FakeBackend::default().polls(vec![
        Err(unanswered()),
        Err(unanswered()),
        Err(unanswered()),
    ]);
    let options = WaitOptions {
        max_consecutive_misses: 2,
        ..fast()
    };
    let (result, phases) = wait(&fake, None, &options).await;
    let err = result.unwrap_err();
    assert!(matches!(err, CyclesPurchaseError::GetOrder { .. }), "{err}");
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 3);
    assert_eq!(phases, vec![PurchasePhase::AwaitingPayment]);
}

/// An order gone for longer than a replica lags is not the caller's, or
/// was forgotten after expiring unpaid: either way, not found.
#[tokio::test]
async fn wait_calls_an_order_gone_for_long_not_found() {
    let fake = FakeBackend::default().polls(vec![Ok(None), Ok(None), Ok(None)]);
    let options = WaitOptions {
        max_consecutive_misses: 2,
        ..fast()
    };
    let (result, phases) = wait(&fake, None, &options).await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::OrderNotFound { .. }
    ));
    assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 3);
    assert_eq!(phases, vec![PurchasePhase::AwaitingPayment]);
}

/// Right after `create_order` a poll can reach a replica that has not seen
/// the order yet; the wait goes on rather than calling the order not found.
#[tokio::test]
async fn wait_takes_a_known_orders_null_for_a_replica_behind() {
    let fake = FakeBackend::default().polls(vec![
        Ok(None),
        Ok(Some(view("co-1", CardOrderStatus::Delivered))),
    ]);
    let initial = initial(CardOrderStatus::AwaitingPayment);
    let (result, _) = wait(&fake, Some(initial), &fast()).await;
    assert_eq!(result.unwrap().status, Some(CardOrderStatus::Delivered));
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
    assert_eq!(result.unwrap().status, Some(CardOrderStatus::Delivered));
    assert_eq!(
        phases,
        vec![PurchasePhase::AwaitingPayment, PurchasePhase::Delivering]
    );
}

/// Given an order already settled, it is returned as it is: no phase is
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
        let settled = initial(status);
        // No poll is scripted: any call would panic the fake.
        let fake = FakeBackend::default();
        let (result, phases) = wait(&fake, Some(settled.clone()), &fast()).await;
        assert_eq!(result.unwrap(), settled);
        assert!(phases.is_empty(), "{phases:?}");
        assert_eq!(fake.count_of(ORDER_STATUS_METHOD), 0);
    }
}

#[tokio::test]
async fn wait_times_out() {
    let fake = FakeBackend::default().polls(vec![Ok(Some(view(
        "co-1",
        CardOrderStatus::AwaitingPayment,
    )))]);
    let options = WaitOptions {
        timeout: Duration::ZERO,
        ..fast()
    };
    let (result, phases) = wait(&fake, None, &options).await;
    match result.unwrap_err() {
        CyclesPurchaseError::WaitTimedOut { order } => {
            assert_eq!(order.status, Some(CardOrderStatus::AwaitingPayment));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(phases, vec![PurchasePhase::AwaitingPayment]);
}

#[tokio::test]
async fn wait_interrupted_returns_last_order() {
    // The interrupt is ready from the start, so whether it or the first poll
    // wins the race, the order handed back is the last one known.
    let fake =
        FakeBackend::default().polls(vec![Ok(Some(view("co-1", CardOrderStatus::Processing)))]);
    let paid = initial(CardOrderStatus::Processing);
    let result = wait_for_order(
        &fake,
        &gateway(),
        "co-1",
        Some(paid),
        &WaitOptions::default(),
        ready(()),
        |_| {},
    )
    .await;
    match result.unwrap_err() {
        CyclesPurchaseError::Interrupted { order } => {
            assert_eq!(order.id, "co-1");
            assert_eq!(order.status, Some(CardOrderStatus::Processing));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn wait_interrupt_lands_during_a_stalled_poll() {
    // The backend never answers; the interrupt must still get through.
    let fake = FakeBackend {
        stall_polls: true,
        ..FakeBackend::default()
    };
    let result = wait_for_order(
        &fake,
        &gateway(),
        "co-1",
        None,
        &WaitOptions::default(),
        ready(()),
        |_| {},
    )
    .await;
    assert!(matches!(
        result.unwrap_err(),
        CyclesPurchaseError::Interrupted { .. }
    ));
}

#[test]
fn statuses_tell_settled_apart() {
    for status in [
        CardOrderStatus::AwaitingPayment,
        CardOrderStatus::PaymentPending,
        CardOrderStatus::Processing,
    ] {
        assert!(!status.is_terminal(), "{status}");
    }
    for status in [
        CardOrderStatus::Delivered,
        CardOrderStatus::NeedsReview,
        CardOrderStatus::Refunding,
        CardOrderStatus::Refunded,
        CardOrderStatus::RefundOwed,
    ] {
        assert!(status.is_terminal(), "{status}");
    }
    assert_eq!(CardOrderStatus::RefundOwed.tag(), "refund_owed");
    assert_eq!(CardOrderStatus::RefundOwed.to_string(), "owed a refund");
    // A state this version does not know is neither settled nor nameless.
    let mut unknown = OrderView::read(view("co-1", CardOrderStatus::Processing));
    unknown.status = None;
    assert!(!unknown.is_settled());
    assert_eq!(unknown.status_tag(), "unknown");
    assert!(unknown.status_text().contains("does not know"));
}
