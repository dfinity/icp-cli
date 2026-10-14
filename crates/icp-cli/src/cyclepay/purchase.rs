//! The purchase itself: quote, create, read back, wait.
//!
//! The buyer is whoever makes the calls: the order is created as the caller,
//! the cycles go to the caller's own Cycles Ledger account, and only the
//! caller can read the order back. The flow is [`connect`] to the gateway,
//! which reads the terms it publishes, quote at its published rate, create
//! the order, pay on the page the gateway hands out for it, then wait for the
//! gateway to report the order delivered.
//!
//! Nothing here prints. [`wait_for_order`] reports its progress as
//! [`icp_events`] tasks, one per [`PurchasePhase`], and hands the last order
//! it saw back inside its error when it is interrupted, so the caller can say
//! where it stands.

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use candid::Principal;
use ic_agent::AgentError;
use icp_events::TaskOutcome;
use icp_project::calls::{
    CallError, CanisterCalls, RouteTo, TypedCallError, query_typed, update_typed,
};
use icp_project::prelude::IC_ROOT_KEY;
use num_traits::ToPrimitive;
use snafu::{OptionExt, Snafu};
use std::fmt;
use tokio::{select, time::sleep};
use url::Url;

use super::task::{CyclesPurchaseTask, PurchasePhase, Reporter, TaskReporter};

use super::interface::{
    CARD_TERMS_METHOD, CREATE_ORDER_METHOD, CardOrderStatus, CardTerms, CreateOrderArgs,
    CreateOrderResult, CreatedOrder, Destination, ORDER_STATUS_METHOD, OrderDestination,
    OrderError, OrderStatusView, XDR_RATE_STATUS_METHOD, XdrRateStatus, is_cyclepay,
};
use super::money::{Currency, Money};
use super::pricing::{Terms, card_fee_cents, least_gross_for, ledger_cycles, net_usd_cents};

/// How often [`wait_for_order`] asks the backend for the order.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Where orders go, and on what terms: the gateway canister that owns them,
/// with the card terms it published when [`connect`] read them. Nothing
/// about how the gateway sells is the CLI's own.
#[derive(Clone, Debug)]
pub struct Gateway {
    pub canister: Principal,
    /// The gateway is taking no new card orders right now.
    pub paused: bool,
    /// The range and the arithmetic every quote is made with.
    pub terms: Terms,
}

impl Gateway {
    /// `canister`, on the terms it published. Terms this CLI could not act
    /// on are refused here, before a quote is made on them: a range with no
    /// amount in it, or a delivery fee beyond any count of cycles.
    pub fn new(canister: Principal, terms: CardTerms) -> Result<Self, CyclesPurchaseError> {
        let invalid = |why: String| CyclesPurchaseError::InvalidTerms {
            gateway: canister,
            why,
        };
        if terms.min_order_usd_cents > terms.max_order_usd_cents {
            return Err(invalid(format!(
                "its order range of {} to {} US cents allows no order",
                terms.min_order_usd_cents, terms.max_order_usd_cents
            )));
        }
        let ledger_delivery_fee_cycles =
            terms
                .ledger_delivery_fee_cycles
                .0
                .to_u128()
                .ok_or_else(|| {
                    invalid(format!(
                        "its ledger delivery fee of {} cycles",
                        terms.ledger_delivery_fee_cycles
                    ))
                })?;
        Ok(Self {
            canister,
            paused: terms.deliveries_paused,
            terms: Terms {
                min_order_usd_cents: terms.min_order_usd_cents,
                max_order_usd_cents: terms.max_order_usd_cents,
                fee_bps: terms.fee_bps,
                fee_fixed_usd_cents: terms.fee_fixed_usd_cents,
                ledger_delivery_fee_cycles,
            },
        })
    }
}

#[derive(Debug, Snafu)]
pub enum CyclesPurchaseError {
    #[snafu(display("the cycles gateway canister {gateway} does not exist on this network"))]
    GatewayNotFound { gateway: Principal },

    #[snafu(display("this network is not mainnet, and {MAINNET_HINT}"))]
    GatewayNotOnThisNetwork { gateway: Principal },

    #[snafu(display("failed to read the card terms from cycles gateway {gateway}"))]
    ReadTerms {
        gateway: Principal,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display(
        "cycles gateway {gateway} published terms this version of icp cannot act on: {why}"
    ))]
    InvalidTerms { gateway: Principal, why: String },

    #[snafu(display(
        "cycles gateway {gateway} is not taking card orders right now; try again later"
    ))]
    CardsPaused { gateway: Principal },

    #[snafu(display("failed to read the exchange rate from cycles gateway {gateway}"))]
    ReadRate {
        gateway: Principal,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display(
        "cycles gateway {gateway} is not pricing orders right now (it has no \
         exchange rate); try again in a few minutes"
    ))]
    RateUnavailable { gateway: Principal },

    #[snafu(display("amount {amount} is outside the gateway's range of {min} to {max} per order"))]
    AmountOutOfRange {
        amount: Money,
        min: Money,
        max: Money,
    },

    #[snafu(display(
        "{} cost more than the gateway maximum of {max} per order, which buys about {}; \
         ask for fewer", super::format_cycles(*cycles), super::format_cycles(*buys)
    ))]
    CyclesAboveMaximum {
        cycles: u128,
        max: Money,
        buys: u128,
    },

    #[snafu(display("failed to create an order on cycles gateway {gateway}"))]
    CreateOrder {
        gateway: Principal,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display("the cycles gateway refused the order: {error}"))]
    OrderRefused { error: OrderError },

    #[snafu(display(
        "cycles gateway {gateway} made order {id} with a payment page that is not an https \
         URL: {url}"
    ))]
    InvalidPaymentUrl {
        gateway: Principal,
        id: String,
        url: String,
    },

    #[snafu(display("failed to read order {id} from cycles gateway {gateway}"))]
    GetOrder {
        gateway: Principal,
        id: String,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display(
        "order {id} was not found on cycles gateway {gateway} for {caller}; \
         an order is visible only to the identity that created it, and an unpaid \
         one is forgotten once it expires"
    ))]
    OrderNotFound {
        gateway: Principal,
        id: String,
        caller: Principal,
    },

    #[snafu(display("interrupted while waiting for order {}", order.id))]
    Interrupted { order: Box<OrderView> },

    #[snafu(display(
        "gave up waiting for order {} (last status: {})", order.id, order.status
    ))]
    WaitTimedOut { order: Box<OrderView> },

    #[snafu(display("lost contact with cycles gateway {gateway} while waiting for order {id}"))]
    WaitUnanswered {
        gateway: Principal,
        id: String,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },
}

/// What the buyer asked for: a sum to spend, or cycles to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Spend exactly this much; take whatever cycles it buys.
    Spend(Money),
    /// Receive at least this many cycles; spend the least `currency` that
    /// buys them.
    Receive { cycles: u128, currency: Currency },
}

/// The backend's XDR/USD rate, as read for a quote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    /// US dollars per XDR (one trillion cycles), times 10,000.
    pub xdr_usd_x10000: u64,
    /// The backend has not refreshed it for longer than it tolerates. Every
    /// order is still priced with it.
    pub stale: bool,
    /// How long ago the backend set it, when it says.
    pub age: Option<Duration>,
}

impl Rate {
    /// The price of one trillion cycles: `1.3520 USD`.
    pub fn usd_per_trillion(&self) -> String {
        format!(
            "{}.{:04} USD",
            self.xdr_usd_x10000 / 10_000,
            self.xdr_usd_x10000 % 10_000
        )
    }
}

/// What an amount buys, at the rate the backend published when asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    /// What the card is charged.
    pub amount: Money,
    /// The card-processing fee included in `amount`.
    pub fee: Money,
    /// What funds the cycles: `amount` less `fee`.
    pub net: Money,
    /// About what lands on the buyer's Cycles Ledger account: the cycles the
    /// net mints at `rate`, less the delivery's ledger fees. The cycles
    /// minted follow the rate when the payment lands.
    pub cycles: u128,
    pub rate: Rate,
}

/// An order's state as the CLI tells it.
///
/// The backend's own states, plus [`OrderStatus::Expired`] for an unpaid
/// order past its deadline: the backend reports such an order as still
/// awaiting payment until it forgets it, but a payment for it is refunded
/// rather than delivered, so it is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    /// Made, not paid.
    AwaitingPayment,
    /// Paid with a delayed method; not settled yet.
    PaymentPending,
    /// Paid; the cycles are on their way.
    Processing,
    Delivered,
    /// Stopped for an operator; not refunded meanwhile.
    NeedsReview,
    /// Not delivered; the refund is being made.
    Refunding,
    Refunded,
    /// Not delivered; an operator refunds it by hand.
    RefundOwed,
    /// Unpaid past its deadline.
    Expired,
    /// A state the backend reports that this CLI does not know. Not an end:
    /// the wait goes on.
    Unknown,
}

impl OrderStatus {
    /// Whether the order can still change in a way worth waiting for. A
    /// refund in progress is settled as far as the buyer is concerned.
    pub fn is_terminal(self) -> bool {
        !matches!(
            self,
            Self::AwaitingPayment | Self::PaymentPending | Self::Processing | Self::Unknown
        )
    }

    /// Whether a payment has been made.
    pub fn is_paid(self) -> bool {
        !matches!(self, Self::AwaitingPayment | Self::Expired | Self::Unknown)
    }

    /// The status for output meant to be parsed, e.g. `refund_owed`.
    /// [`fmt::Display`] is for people.
    pub fn tag(self) -> &'static str {
        match self {
            Self::AwaitingPayment => "awaiting_payment",
            Self::PaymentPending => "payment_pending",
            Self::Processing => "processing",
            Self::Delivered => "delivered",
            Self::NeedsReview => "needs_review",
            Self::Refunding => "refunding",
            Self::Refunded => "refunded",
            Self::RefundOwed => "refund_owed",
            Self::Expired => "expired",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AwaitingPayment => "awaiting payment",
            Self::PaymentPending => "paid, payment pending",
            Self::Processing => "paid, delivering",
            Self::Delivered => "delivered",
            Self::NeedsReview => "needs review",
            Self::Refunding => "being refunded",
            Self::Refunded => "refunded",
            Self::RefundOwed => "owed a refund",
            Self::Expired => "expired",
            Self::Unknown => "in a state this version of icp does not know",
        })
    }
}

impl From<CardOrderStatus> for OrderStatus {
    fn from(status: CardOrderStatus) -> Self {
        match status {
            CardOrderStatus::AwaitingPayment => Self::AwaitingPayment,
            CardOrderStatus::PaymentPending => Self::PaymentPending,
            CardOrderStatus::Processing => Self::Processing,
            CardOrderStatus::Delivered => Self::Delivered,
            CardOrderStatus::NeedsReview => Self::NeedsReview,
            CardOrderStatus::Refunding => Self::Refunding,
            CardOrderStatus::Refunded => Self::Refunded,
            CardOrderStatus::RefundOwed => Self::RefundOwed,
        }
    }
}

/// An order, as the CLI holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderView {
    pub id: String,
    pub status: OrderStatus,
    /// What the order asked for.
    pub amount: Money,
    /// What funds the cycles after the card fee: the order's amount until
    /// paid, then the amount paid.
    pub net: Money,
    /// The page to pay on, while the order is payable.
    pub checkout_url: Option<String>,
    /// When the order stops being payable, in seconds since the Unix epoch.
    /// `None` once paid.
    pub expires_at: Option<u64>,
    /// What the card was charged, once paid.
    pub paid: Option<Money>,
    /// Exact cycles, once delivered.
    pub cycles_delivered: Option<u128>,
    /// What was refunded, once anything was.
    pub refunded: Option<Money>,
    /// Why delivery waits or stopped, or why a refund is owed.
    pub detail: Option<String>,
    /// Where the cycles go, when the backend says in a way this CLI knows.
    pub destination: Option<Destination>,
}

impl OrderView {
    /// An order just made by `buyer`, for its own Cycles Ledger account, to
    /// be paid on the page the gateway handed out with it.
    fn created(created: CreatedOrder, buyer: Principal) -> Self {
        Self {
            id: created.order_id,
            status: OrderStatus::AwaitingPayment,
            amount: Money::from_minor_units(created.amount_usd_cents, Currency::Usd),
            net: Money::from_minor_units(created.net_usd_cents, Currency::Usd),
            checkout_url: Some(created.payment_url),
            expires_at: Some(created.expires_at),
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
            destination: Some(Destination::Ledger(buyer)),
        }
    }

    /// The view the backend gives the owner. The page to pay on is not in
    /// it: an order comes with its page once, when it is made.
    pub(super) fn read(view: OrderStatusView) -> Self {
        Self {
            id: view.order_id,
            status: view.status.map_or(OrderStatus::Unknown, OrderStatus::from),
            amount: Money::from_minor_units(view.amount_usd_cents, Currency::Usd),
            net: Money::from_minor_units(view.net_usd_cents, Currency::Usd),
            checkout_url: None,
            expires_at: view.expires_at,
            paid: view
                .paid_usd_cents
                .map(|cents| Money::from_minor_units(cents, Currency::Usd)),
            cycles_delivered: view.cycles_delivered.map(u128::from),
            refunded: (view.amount_refunded_usd_cents > 0)
                .then(|| Money::from_minor_units(view.amount_refunded_usd_cents, Currency::Usd)),
            detail: view.detail,
            destination: view.destination,
        }
    }

    /// The same order, over: unpaid past its deadline.
    fn expired(mut self) -> Self {
        self.status = OrderStatus::Expired;
        self.checkout_url = None;
        self
    }

    /// Whether the order is unpaid and `now` (seconds since the Unix epoch)
    /// is more than `grace` past its deadline.
    fn is_past_deadline(&self, now: u64, grace: Duration) -> bool {
        self.status == OrderStatus::AwaitingPayment
            && self
                .expires_at
                .is_some_and(|deadline| now > deadline.saturating_add(grace.as_secs()))
    }
}

/// Reach `canister` as a cycles gateway: read the card terms it publishes,
/// and hold them for every call after. A canister that is not there is
/// [`CyclesPurchaseError::GatewayNotFound`], said here before anything else
/// is asked of it.
pub async fn connect(
    calls: &dyn CanisterCalls,
    canister: Principal,
) -> Result<Gateway, CyclesPurchaseError> {
    let (terms,): (CardTerms,) = query_typed(calls, canister, CARD_TERMS_METHOD, ())
        .await
        .map_err(|source| {
            map_absent(source, canister, |source| CyclesPurchaseError::ReadTerms {
                gateway: canister,
                source: Box::new(source),
            })
        })?;
    Gateway::new(canister, terms)
}

/// Read the rate the backend prices orders with right now.
pub async fn read_rate(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
) -> Result<Rate, CyclesPurchaseError> {
    let canister = gateway.canister;
    let (status,): (XdrRateStatus,) = query_typed(calls, canister, XDR_RATE_STATUS_METHOD, ())
        .await
        .map_err(|source| {
            map_absent(source, canister, |source| CyclesPurchaseError::ReadRate {
                gateway: canister,
                source: Box::new(source),
            })
        })?;
    if status.xdr_usd_x10000 == 0 {
        return RateUnavailableSnafu { gateway: canister }.fail();
    }
    Ok(Rate {
        xdr_usd_x10000: status.xdr_usd_x10000,
        stale: status.stale,
        age: status.age_seconds.map(Duration::from_secs),
    })
}

/// Price a [`Target`] at `rate`, on the gateway's terms and within its
/// per-order range.
///
/// A sum outside the range, or any order while the gateway is paused, is
/// refused here rather than by the gateway so the buyer is told before being
/// shown a quote for it. A number of cycles the minimum already buys is
/// quoted at the minimum: that is the least a buyer can pay, and the extra is
/// theirs.
pub fn quote_at(
    gateway: &Gateway,
    rate: Rate,
    target: Target,
) -> Result<Quote, CyclesPurchaseError> {
    if gateway.paused {
        return CardsPausedSnafu {
            gateway: gateway.canister,
        }
        .fail();
    }
    let terms = &gateway.terms;
    let amount = match target {
        Target::Spend(amount) => {
            let Currency::Usd = amount.currency;
            let min = Money::from_minor_units(terms.min_order_usd_cents, amount.currency);
            let max = Money::from_minor_units(terms.max_order_usd_cents, amount.currency);
            if amount.minor_units < min.minor_units || amount.minor_units > max.minor_units {
                return AmountOutOfRangeSnafu { amount, min, max }.fail();
            }
            amount
        }
        Target::Receive { cycles, currency } => {
            let Currency::Usd = currency;
            let cents = least_gross_for(cycles, rate.xdr_usd_x10000, terms).context(
                CyclesAboveMaximumSnafu {
                    cycles,
                    max: Money::from_minor_units(terms.max_order_usd_cents, currency),
                    buys: ledger_cycles(terms.max_order_usd_cents, rate.xdr_usd_x10000, terms),
                },
            )?;
            Money::from_minor_units(cents, currency)
        }
    };
    Ok(Quote {
        amount,
        fee: Money::from_minor_units(card_fee_cents(amount.minor_units, terms), amount.currency),
        net: Money::from_minor_units(net_usd_cents(amount.minor_units, terms), amount.currency),
        cycles: ledger_cycles(amount.minor_units, rate.xdr_usd_x10000, terms),
        rate,
    })
}

/// Read the backend's rate and price a [`Target`] at it.
pub async fn quote_target(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    target: Target,
) -> Result<Quote, CyclesPurchaseError> {
    let rate = read_rate(calls, gateway).await?;
    quote_at(gateway, rate, target)
}

/// Create an order for `amount`, delivering to the caller's own Cycles
/// Ledger account.
///
/// The backend answers an open order of the caller's for the same amount and
/// destination again rather than making a second one, so repeating a run
/// resumes it.
pub async fn place_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    amount: Money,
) -> Result<OrderView, CyclesPurchaseError> {
    let Currency::Usd = amount.currency;
    let args = CreateOrderArgs {
        destination: OrderDestination::Ledger,
        beneficiary: None,
        project_id: None,
        amount_usd_cents: amount.minor_units,
        confirm_system_canister: None,
    };
    let canister = gateway.canister;
    let (result,): (CreateOrderResult,) = update_typed(
        calls,
        canister,
        CREATE_ORDER_METHOD,
        (args,),
        RouteTo::Callee,
        0,
    )
    .await
    .map_err(|source| {
        map_absent(source, canister, |source| {
            CyclesPurchaseError::CreateOrder {
                gateway: canister,
                source: Box::new(source),
            }
        })
    })?;
    match result {
        // The page is the gateway's to hand out, and is opened as it is;
        // a card is typed on it, so it must at least be an https URL.
        CreateOrderResult::Ok(created)
            if !Url::parse(&created.payment_url).is_ok_and(|url| url.scheme() == "https") =>
        {
            InvalidPaymentUrlSnafu {
                gateway: canister,
                id: created.order_id,
                url: created.payment_url,
            }
            .fail()
        }
        CreateOrderResult::Ok(created) => Ok(OrderView::created(created, calls.caller())),
        CreateOrderResult::Err(error) => OrderRefusedSnafu { error }.fail(),
    }
}

/// Read one of the caller's orders, or `None` where the backend answers
/// `null`: an order the caller does not own, one that never existed, or one
/// forgotten after expiring unpaid.
async fn fetch_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    id: &str,
) -> Result<Option<OrderView>, CyclesPurchaseError> {
    let canister = gateway.canister;
    let (view,): (Option<OrderStatusView>,) =
        query_typed(calls, canister, ORDER_STATUS_METHOD, (id.to_owned(),))
            .await
            .map_err(|source| {
                map_absent(source, canister, |source| CyclesPurchaseError::GetOrder {
                    gateway: canister,
                    id: id.to_owned(),
                    source: Box::new(source),
                })
            })?;
    Ok(view.map(OrderView::read))
}

/// Bounds on [`wait_for_order`].
#[derive(Clone, Debug)]
pub struct WaitOptions {
    /// Time between polls.
    pub poll_interval: Duration,
    /// How far past its deadline an unpaid order is still read as payable.
    /// Clocks differ, and the backend expires it on its own.
    pub expiry_grace: Duration,
    /// How long an unpaid order is waited on in one run. Its deadline is a
    /// day away; a buyer who has not paid by now is told where it stands.
    pub max_unpaid_wait: Duration,
    /// How long a paid order is given to be delivered.
    pub max_paid_wait: Duration,
    /// Consecutive polls that could not be answered tolerated before giving
    /// up: ten minutes of them at [`POLL_INTERVAL`], longer than the few
    /// minutes CyclePay's backend is stopped for during an upgrade.
    pub max_consecutive_transient: u32,
    /// Consecutive polls answering `null` for an order already seen, before
    /// its deadline, tolerated before it is called not found: a minute of
    /// them at [`POLL_INTERVAL`], where a replica that has not caught up
    /// needs seconds.
    pub max_consecutive_missing: u32,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            expiry_grace: Duration::from_secs(60),
            max_unpaid_wait: Duration::from_secs(60 * 60),
            max_paid_wait: Duration::from_secs(15 * 60),
            max_consecutive_transient: 200,
            max_consecutive_missing: 20,
        }
    }
}

/// The legs of a wait, reported as one [`CyclesPurchaseTask`] each so a
/// consumer can tick off "payment received" before "delivered".
struct Phases<'a> {
    reporter: &'a Reporter,
    id: &'a str,
    current: Option<(PurchasePhase, TaskReporter)>,
}

impl Phases<'_> {
    fn of(status: OrderStatus) -> PurchasePhase {
        match status {
            OrderStatus::PaymentPending | OrderStatus::Processing => PurchasePhase::Delivering,
            _ => PurchasePhase::AwaitingPayment,
        }
    }

    /// Move to the phase `status` is in, finishing the one before it. A
    /// state this CLI cannot place leaves the phase as it is.
    fn enter(&mut self, status: OrderStatus) {
        if status == OrderStatus::Unknown && self.current.is_some() {
            return;
        }
        let phase = Self::of(status);
        if self.current.as_ref().is_some_and(|(p, _)| *p == phase) {
            return;
        }
        self.finish(TaskOutcome::succeeded());
        let task = self.reporter.task(CyclesPurchaseTask::new(self.id, phase));
        self.current = Some((phase, task));
    }

    fn finish(&mut self, outcome: TaskOutcome) {
        if let Some((_, task)) = self.current.take() {
            task.finish(outcome);
        }
    }
}

/// Poll the backend until the order reaches a terminal status.
///
/// `initial` is the order as last seen, if the caller has it: it names the
/// phase the wait starts in, and it is what
/// [`CyclesPurchaseError::Interrupted`] carries if `interrupt` resolves before
/// the first poll answers. Each [`PurchasePhase`] the order passes through is
/// reported as a task on `reporter`, finished when the next begins or the
/// wait ends.
///
/// `interrupt` is raced against every poll as well as the sleep between
/// them, so an interrupt lands even while a call is stalled.
///
/// A poll the network never answered is retried; any other failure returns at
/// once. An unpaid order past its deadline comes back as
/// [`OrderStatus::Expired`], whether the backend still reports it or has
/// forgotten it. A terminal status is a success here whatever it is — a
/// refund is an answer — and the caller decides what each one means.
pub async fn wait_for_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    id: &str,
    initial: Option<OrderView>,
    options: &WaitOptions,
    interrupt: impl Future<Output = ()>,
    reporter: &Reporter,
) -> Result<OrderView, CyclesPurchaseError> {
    // An order already settled has nothing to wait for: no phase to show,
    // and no call that could turn a known end into an error.
    if let Some(order) = initial.as_ref().filter(|order| order.status.is_terminal()) {
        return Ok(order.clone());
    }
    let mut interrupt = pin!(interrupt);
    let mut phases = Phases {
        reporter,
        id,
        current: None,
    };
    phases.enter(
        initial
            .as_ref()
            .map_or(OrderStatus::AwaitingPayment, |o| o.status),
    );
    let mut last = initial;
    let mut transient_streak = 0u32;
    let mut missing_streak = 0u32;
    let started = Instant::now();
    let mut paid_since: Option<Instant> = None;

    let interrupted = |last: Option<OrderView>| {
        let order = last.unwrap_or_else(|| OrderView {
            id: id.to_owned(),
            status: OrderStatus::AwaitingPayment,
            amount: Money::from_minor_units(0, Currency::Usd),
            net: Money::from_minor_units(0, Currency::Usd),
            checkout_url: None,
            expires_at: None,
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
            destination: None,
        });
        InterruptedSnafu {
            order: Box::new(order),
        }
        .fail()
    };

    loop {
        let polled = select! {
            polled = fetch_order(calls, gateway, id) => polled,
            _ = &mut interrupt => {
                phases.finish(TaskOutcome::Stopped {
                    message: "stopped waiting".to_owned(),
                });
                return interrupted(last);
            }
        };
        match polled {
            Ok(Some(order)) => {
                transient_streak = 0;
                missing_streak = 0;
                let order = if order.is_past_deadline(now_secs(), options.expiry_grace) {
                    order.expired()
                } else {
                    order
                };
                if order.status.is_terminal() {
                    // A delivery first seen at its end still went through
                    // payment: both legs are ticked off, as for any other.
                    if order.status == OrderStatus::Delivered {
                        phases.enter(OrderStatus::Processing);
                    }
                    phases.finish(match order.status {
                        OrderStatus::Delivered => TaskOutcome::succeeded(),
                        status => TaskOutcome::failed(format!("order {status}")),
                    });
                    return Ok(order);
                }
                phases.enter(order.status);
                if order.status.is_paid() && paid_since.is_none() {
                    paid_since = Some(Instant::now());
                }
                let overdue = match paid_since {
                    Some(since) => since.elapsed() > options.max_paid_wait,
                    None => started.elapsed() > options.max_unpaid_wait,
                };
                if overdue {
                    phases.finish(TaskOutcome::failed("gave up waiting"));
                    return WaitTimedOutSnafu {
                        order: Box::new(order),
                    }
                    .fail();
                }
                last = Some(order);
            }
            // The backend forgets an unpaid order once it expires. One that
            // was awaiting payment and is now gone past its deadline expired;
            // one gone for any other reason was never the caller's to wait on.
            Ok(None) => match last.take() {
                Some(order) if order.is_past_deadline(now_secs(), Duration::ZERO) => {
                    let order = order.expired();
                    phases.finish(TaskOutcome::failed(format!("order {}", order.status)));
                    return Ok(order);
                }
                // The backend forgets an order only once it expires, so a
                // null before then is a replica that has not caught up with
                // it (a poll right after `create_order` can reach one): tried
                // again, for as long as a replica lags.
                Some(order) if missing_streak < options.max_consecutive_missing => {
                    missing_streak += 1;
                    last = Some(order);
                }
                _ => {
                    phases.finish(TaskOutcome::failed("order not found"));
                    return OrderNotFoundSnafu {
                        gateway: gateway.canister,
                        id,
                        caller: calls.caller(),
                    }
                    .fail();
                }
            },
            Err(CyclesPurchaseError::GetOrder { source, .. }) if is_transient(&source) => {
                transient_streak += 1;
                if transient_streak > options.max_consecutive_transient {
                    phases.finish(TaskOutcome::failed("lost contact with the gateway"));
                    return Err(CyclesPurchaseError::WaitUnanswered {
                        gateway: gateway.canister,
                        id: id.to_owned(),
                        source,
                    });
                }
            }
            Err(error) => {
                phases.finish(TaskOutcome::failed(error.to_string()));
                return Err(error);
            }
        }

        select! {
            _ = sleep(options.poll_interval) => {}
            _ = &mut interrupt => {
                phases.finish(TaskOutcome::Stopped {
                    message: "stopped waiting".to_owned(),
                });
                return interrupted(last);
            }
        }
    }
}

/// Where to buy instead, said whenever CyclePay's own gateway cannot be
/// reached on the network chosen.
pub const MAINNET_HINT: &str = "CyclePay sells cycles on mainnet: use `-n ic`";

/// Refuse to buy through CyclePay's own gateway anywhere but mainnet, told
/// apart by `root_key`, the key the agent verifies the network's answers
/// with. Any other gateway could be on any network, and is left to say for
/// itself whether it is there.
///
/// Checked before the gateway is asked anything, so the answer does not
/// depend on how a network reports a canister it does not have, and the
/// same thing is said however the network was chosen.
pub fn check_network(gateway: Principal, root_key: &[u8]) -> Result<(), CyclesPurchaseError> {
    if is_cyclepay(gateway) && root_key != IC_ROOT_KEY.as_slice() {
        return GatewayNotOnThisNetworkSnafu { gateway }.fail();
    }
    Ok(())
}

/// An answer saying the gateway canister does not exist becomes
/// [`CyclesPurchaseError::GatewayNotFound`]; anything else goes to `wrap`.
///
/// A replica says so with an IC0301 rejection. An HTTP gateway that cannot
/// route to the canister says so itself, before any replica sees the call,
/// with a 4xx whose body is its `canister_not_found`; `AgentCalls` reports
/// that as a failure carrying the agent's own error. Only this command reads
/// the second form as absence: elsewhere in icp-cli a call that no replica
/// answered stays a failure.
fn map_absent(
    source: TypedCallError,
    gateway: Principal,
    wrap: impl FnOnce(TypedCallError) -> CyclesPurchaseError,
) -> CyclesPurchaseError {
    match &source {
        TypedCallError::Call { source: call } if is_absent(call) => {
            CyclesPurchaseError::GatewayNotFound { gateway }
        }
        _ => wrap(source),
    }
}

fn is_absent(call: &CallError) -> bool {
    match call {
        CallError::Failed { source, .. } => matches!(
            source.downcast_ref::<AgentError>(),
            Some(AgentError::HttpError(payload))
                if (400..500).contains(&payload.status)
                    && String::from_utf8_lossy(&payload.content).contains("canister_not_found")
        ),
        _ => call.is_canister_not_found(),
    }
}

/// The replica's error codes for a call to a canister that is stopped or
/// stopping, as CyclePay's backend is for a few minutes during each upgrade.
const STOPPED_CODES: [&str; 2] = ["IC0508", "IC0509"];

/// Whether a failed read of an order is worth repeating. Reading changes
/// nothing, so it always is when the answer could come out differently: the
/// network did not answer; the backend is stopped for an upgrade; or a
/// boundary node was overloaded or could not reach the subnet (an HTTP 429
/// or 5xx, which `icp_app::calls::AgentCalls` reports as a failure carrying
/// the agent's own error).
fn is_transient(error: &TypedCallError) -> bool {
    let TypedCallError::Call { source } = error else {
        return false;
    };
    let overloaded = match source.as_ref() {
        CallError::Failed { source, .. } => matches!(
            source.downcast_ref::<AgentError>(),
            Some(AgentError::HttpError(payload)) if payload.status == 429 || payload.status >= 500
        ),
        _ => false,
    };
    source.is_transient()
        || source
            .code()
            .is_some_and(|code| STOPPED_CODES.contains(&code))
        || overloaded
}

/// Seconds since the Unix epoch, on this machine's clock.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cyclepay::interface::CYCLES_GATEWAY_CID;

    const OTHER_ROOT_KEY: &[u8] = b"not the mainnet root key";

    #[test]
    fn cyclepay_gateway_is_refused_off_mainnet() {
        let cyclepay = Principal::from_text(CYCLES_GATEWAY_CID).unwrap();
        assert!(check_network(cyclepay, IC_ROOT_KEY).is_ok());
        let error = check_network(cyclepay, OTHER_ROOT_KEY).unwrap_err();
        assert!(
            matches!(error, CyclesPurchaseError::GatewayNotOnThisNetwork { gateway } if gateway == cyclepay)
        );
        assert!(error.to_string().contains(MAINNET_HINT), "{error}");
    }

    #[test]
    fn another_gateway_may_be_on_any_network() {
        let other = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        assert!(check_network(other, IC_ROOT_KEY).is_ok());
        assert!(check_network(other, OTHER_ROOT_KEY).is_ok());
    }
}
