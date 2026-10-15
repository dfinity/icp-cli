//! The purchase itself: quote, create, read back, wait.
//!
//! The buyer is whoever makes the calls: the order is created as the caller,
//! the cycles go to the caller's own Cycles Ledger account, and only the
//! caller can read the order back. The flow is [`connect`] to the gateway,
//! which reads the terms it publishes, quote at its published rate, create
//! the order, pay on the page the gateway hands out for it, then wait for the
//! gateway to report the order delivered.
//!
//! Nothing here prints. [`wait_for_order`] tells its caller each
//! [`PurchasePhase`] the order enters, and when the gateway stops answering
//! and starts again, and hands the last order it saw back inside its error
//! when it ends unsettled, so the caller can say where the order stands.

use std::future::Future;
use std::pin::{Pin, pin};
use std::time::Duration;

use candid::Principal;
use ic_agent::AgentError;
use icp_project::calls::{
    CallError, CanisterCalls, RouteTo, TypedCallError, query_typed, update_typed,
};
use icp_project::prelude::IC_ROOT_KEY;
use num_traits::ToPrimitive;
use snafu::{OptionExt, Snafu};
use std::fmt;
use tokio::{
    select,
    time::{Instant, sleep_until, timeout},
};
use url::Url;

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
    #[snafu(display("{gateway} cannot be a cycles gateway: it is {what}"))]
    NotACanister {
        gateway: Principal,
        what: &'static str,
    },

    #[snafu(display("the cycles gateway canister {gateway} does not exist on this network"))]
    GatewayNotFound { gateway: Principal },

    #[snafu(display(
        "canister {gateway} is not a cycles gateway: it does not serve {method}, the method a \
         gateway publishes its terms with"
    ))]
    NotAGateway {
        gateway: Principal,
        method: &'static str,
    },

    #[snafu(display("this network is not mainnet. {MAINNET_HINT}"))]
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

    #[snafu(display(
        "cycles gateway {gateway} did not answer a poll of order {id} within {}s",
        after.as_secs()
    ))]
    PollTimedOut {
        gateway: Principal,
        id: String,
        after: Duration,
    },

    #[snafu(display("interrupted while waiting for order {}", order.id))]
    Interrupted { order: Box<OrderView> },

    /// Interrupted before the gateway answered `create_order`: the order may
    /// or may not have been made.
    #[snafu(display(
        "interrupted before cycles gateway {gateway} answered whether it made the order"
    ))]
    InterruptedPlacing { gateway: Principal },

    #[snafu(display(
        "gave up waiting for order {} (last status: {})", order.id, order.status_text()
    ))]
    WaitTimedOut { order: Box<OrderView> },

    /// Polls kept failing until the wait gave up. `order` is the order as
    /// last seen, and `source` the last poll's failure.
    #[snafu(display(
        "lost contact with cycles gateway {gateway} while waiting for order {}",
        order.id
    ))]
    ContactLost {
        gateway: Principal,
        order: Box<OrderView>,
        source: Box<CyclesPurchaseError>,
    },
}

/// What the buyer asked for: a sum to spend, or cycles to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Spend exactly this much; take whatever cycles it buys.
    Spend(Money),
    /// Receive this many cycles: spend the least `currency` whose estimate,
    /// on the gateway's published arithmetic at the rate quoted, covers them.
    /// What lands follows the gateway's own conversion at the rate in force
    /// when the payment lands, so it can fall short of the estimate by a
    /// little.
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

impl CardOrderStatus {
    /// Whether the order can still change in a way worth waiting for. A
    /// refund in progress is settled as far as the buyer is concerned.
    pub fn is_terminal(self) -> bool {
        !matches!(
            self,
            Self::AwaitingPayment | Self::PaymentPending | Self::Processing
        )
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
        }
    }
}

impl fmt::Display for CardOrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AwaitingPayment => "awaiting payment",
            Self::PaymentPending => "paid by a method that has not settled yet",
            Self::Processing => "paid, delivering",
            Self::Delivered => "delivered",
            Self::NeedsReview => "needs review",
            Self::Refunding => "being refunded",
            Self::Refunded => "refunded",
            Self::RefundOwed => "owed a refund",
        })
    }
}

/// An order, as the CLI holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderView {
    pub id: String,
    /// `None` for a state the backend reports that this CLI does not know.
    /// Not an end: the wait goes on.
    pub status: Option<CardOrderStatus>,
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
    /// Whether the order can still change in a way worth waiting for.
    pub fn is_settled(&self) -> bool {
        self.status.is_some_and(CardOrderStatus::is_terminal)
    }

    /// The status for people: `owed a refund`.
    pub fn status_text(&self) -> String {
        match self.status {
            Some(status) => status.to_string(),
            None => "in a state this version of icp does not know".to_owned(),
        }
    }

    /// The status for output meant to be parsed: `refund_owed`.
    pub fn status_tag(&self) -> &'static str {
        self.status.map_or("unknown", CardOrderStatus::tag)
    }

    /// An order just made by `buyer`, for its own Cycles Ledger account, to
    /// be paid on the page the gateway handed out with it.
    fn created(created: CreatedOrder, buyer: Principal) -> Self {
        Self {
            id: created.order_id,
            status: Some(CardOrderStatus::AwaitingPayment),
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
            status: view.status,
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
        .map_err(|source| match &source {
            // A canister that is there but serves no such method: not a
            // gateway at all, which the call's own words would only hint at.
            TypedCallError::Call { source: call } if is_method_not_found(call) => {
                CyclesPurchaseError::NotAGateway {
                    gateway: canister,
                    method: CARD_TERMS_METHOD,
                }
            }
            _ => map_absent(source, canister, |source| CyclesPurchaseError::ReadTerms {
                gateway: canister,
                source: Box::new(source),
            }),
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
///
/// `interrupt` is raced against the call, which a stalled network can hold
/// for minutes: if it comes first, the call is abandoned with
/// [`CyclesPurchaseError::InterruptedPlacing`], and whether the order was
/// made is not known.
pub async fn place_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    amount: Money,
    interrupt: impl Future<Output = ()>,
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
    let call = update_typed(
        calls,
        canister,
        CREATE_ORDER_METHOD,
        (args,),
        RouteTo::Callee,
        0,
    );
    // An answer that comes with the interrupt is taken: the order is known.
    let answered = select! {
        biased;
        answered = call => answered,
        _ = interrupt => return InterruptedPlacingSnafu { gateway: canister }.fail(),
    };
    let (result,): (CreateOrderResult,) = answered.map_err(|source| {
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

/// Bounds on [`wait_for_order`]. The two that judge silence count time, not
/// polls, so that they hold whether a network refuses a poll at once or
/// stalls it until [`WaitOptions::poll_timeout`].
#[derive(Clone, Debug)]
pub struct WaitOptions {
    /// Time between polls.
    pub poll_interval: Duration,
    /// How long one poll may go unanswered before it counts as failed: a
    /// connection that stalls rather than drops would otherwise hold the
    /// wait, and a poll sent after the network comes back would wait behind
    /// it.
    pub poll_timeout: Duration,
    /// How long since a poll was last answered before the caller is told the
    /// gateway has stopped answering, a poll in flight or not. The wait keeps
    /// trying.
    pub quiet_after: Duration,
    /// How long since a poll last found the order before the wait gives up
    /// on it: polls unanswered, refused, or answered with no order. Longer
    /// than CyclePay's backend is stopped for during an upgrade.
    pub give_up_after: Duration,
    /// How long the order is waited on in one run. Its deadline is a day
    /// away; a buyer who has not paid by now is told where it stands.
    pub timeout: Duration,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            poll_timeout: Duration::from_secs(10),
            quiet_after: Duration::from_secs(9),
            give_up_after: Duration::from_secs(5 * 60),
            timeout: Duration::from_secs(60 * 60),
        }
    }
}

/// Which leg of a card purchase of cycles is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchasePhase {
    /// The order is payable and the buyer has not paid yet.
    AwaitingPayment,
    /// Paid with a method that settles later, such as a bank debit: not
    /// settled yet, and it can still fail, which makes the order payable
    /// again.
    PaymentPending,
    /// The payment was received; the gateway is sending the cycles.
    Delivering,
}

/// What [`wait_for_order`] tells its caller while it waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitEvent {
    /// The order entered `phase`, or the gateway's `note` on it (its
    /// `detail`: why delivery waits, say) changed. The first is told before
    /// the first poll.
    Order {
        phase: PurchasePhase,
        note: Option<String>,
    },
    /// No poll has been answered for [`WaitOptions::quiet_after`]: the
    /// network, or the gateway, is not answering. The wait keeps trying.
    ContactLost,
    /// A poll was answered again after [`WaitEvent::ContactLost`].
    ContactRestored,
}

/// What the wait has told its caller, so that each change is told once.
struct Told<F> {
    phase: Option<PurchasePhase>,
    note: Option<String>,
    quiet: bool,
    on_event: F,
}

impl<F: FnMut(WaitEvent)> Told<F> {
    fn phase_of(status: Option<CardOrderStatus>) -> PurchasePhase {
        match status {
            Some(CardOrderStatus::PaymentPending) => PurchasePhase::PaymentPending,
            Some(CardOrderStatus::Processing) => PurchasePhase::Delivering,
            _ => PurchasePhase::AwaitingPayment,
        }
    }

    /// Move to the phase `status` is in, with `note`. A state this CLI
    /// cannot place leaves both as they are.
    fn order(&mut self, status: Option<CardOrderStatus>, note: Option<&str>) {
        if status.is_none() && self.phase.is_some() {
            return;
        }
        let phase = Self::phase_of(status);
        if self.phase != Some(phase) || self.note.as_deref() != note {
            self.phase = Some(phase);
            self.note = note.map(str::to_owned);
            (self.on_event)(WaitEvent::Order {
                phase,
                note: self.note.clone(),
            });
        }
    }

    fn quiet(&mut self) {
        self.quiet = true;
        (self.on_event)(WaitEvent::ContactLost);
    }

    fn answered(&mut self) {
        if self.quiet {
            self.quiet = false;
            (self.on_event)(WaitEvent::ContactRestored);
        }
    }
}

/// How a step of the wait (a poll, or the pause after one) ended.
enum Step<T> {
    Done(T),
    Interrupted,
    /// [`WaitOptions::give_up_after`] passed with no word of the order.
    GaveUp,
}

/// Run `step` to its end, unless `interrupt` or the time to give up comes
/// first, telling `told` once the gateway has gone unanswered past
/// `quiet_at`.
async fn watch<T, F: FnMut(WaitEvent)>(
    step: impl Future<Output = T>,
    interrupt: &mut Pin<&mut impl Future<Output = ()>>,
    quiet_at: Instant,
    give_up_at: Instant,
    told: &mut Told<F>,
) -> Step<T> {
    let mut step = pin!(step);
    loop {
        // In this order when more than one is due: a stop always lands, and
        // a step due at the moment the gateway goes quiet is taken first,
        // so that an answer then is not told as silence.
        select! {
            biased;
            _ = interrupt.as_mut() => return Step::Interrupted,
            _ = sleep_until(give_up_at) => return Step::GaveUp,
            done = &mut step => return Step::Done(done),
            _ = sleep_until(quiet_at), if !told.quiet => told.quiet(),
        }
    }
}

/// Poll the backend until the order reaches a terminal status.
///
/// `initial` is the order as last seen, if the caller has it: it names the
/// phase the wait starts in, and it is what
/// [`CyclesPurchaseError::Interrupted`] carries if `interrupt` resolves before
/// the first poll answers. `on_event` is told each [`PurchasePhase`] the
/// order enters, the first before the first poll, the gateway's note on it,
/// and when the gateway stops and starts answering.
///
/// `interrupt` is raced against every poll as well as the pause between
/// them, so an interrupt lands even while a call is stalled.
///
/// A poll that says nothing of the order is tried again: one the network
/// never answered, one the backend refused (as it does for a few minutes
/// during an upgrade), or one answered `null` (a replica that has not caught
/// up with a new order, or an unpaid order forgotten once it expired). A poll
/// gets [`WaitOptions::poll_timeout`] to answer. Once the polls have said
/// nothing of the order for [`WaitOptions::give_up_after`], the wait gives
/// up: with [`CyclesPurchaseError::ContactLost`], carrying the order as last
/// seen, if the last poll failed, and [`CyclesPurchaseError::OrderNotFound`]
/// if it was answered. A terminal status is a success here whatever it is
/// (a refund is an answer), and the caller decides what each one means.
pub async fn wait_for_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    id: &str,
    initial: Option<OrderView>,
    options: &WaitOptions,
    interrupt: impl Future<Output = ()>,
    on_event: impl FnMut(WaitEvent),
) -> Result<OrderView, CyclesPurchaseError> {
    // An order already settled has nothing to wait for: no phase to show,
    // and no call that could turn a known end into an error.
    if let Some(order) = initial.as_ref().filter(|order| order.is_settled()) {
        return Ok(order.clone());
    }
    let mut interrupt = pin!(interrupt);
    let mut told = Told {
        phase: None,
        note: None,
        quiet: false,
        on_event,
    };
    match &initial {
        Some(order) => told.order(order.status, order.detail.as_deref()),
        None => told.order(Some(CardOrderStatus::AwaitingPayment), None),
    }
    let mut last = initial;
    let started = Instant::now();
    // When a poll was last answered, and last found the order.
    let mut answered = started;
    let mut found = started;
    // What the last poll to end said, when it said nothing of the order: a
    // failure, or `None` for an answer with no order in it.
    let mut missed: Option<Option<CyclesPurchaseError>> = None;

    let known = |last: Option<OrderView>| {
        last.unwrap_or_else(|| OrderView {
            id: id.to_owned(),
            status: Some(CardOrderStatus::AwaitingPayment),
            amount: Money::from_minor_units(0, Currency::Usd),
            net: Money::from_minor_units(0, Currency::Usd),
            checkout_url: None,
            expires_at: None,
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
            destination: None,
        })
    };
    let interrupted = |last: Option<OrderView>| {
        InterruptedSnafu {
            order: Box::new(known(last)),
        }
        .fail()
    };
    let poll_timed_out = || PollTimedOutSnafu {
        gateway: gateway.canister,
        id,
        after: options.poll_timeout,
    };
    // Given up on: out of contact if the last poll failed, or the poll in
    // flight when no poll has ended since the order was last found; not
    // found if the gateway answered that it has no such order.
    let gave_up =
        |last: Option<OrderView>, missed: Option<Option<CyclesPurchaseError>>| match missed
            .unwrap_or_else(|| Some(poll_timed_out().build()))
        {
            Some(source) => Err(CyclesPurchaseError::ContactLost {
                gateway: gateway.canister,
                order: Box::new(known(last)),
                source: Box::new(source),
            }),
            None => OrderNotFoundSnafu {
                gateway: gateway.canister,
                id,
                caller: calls.caller(),
            }
            .fail(),
        };

    loop {
        let poll = async {
            match timeout(options.poll_timeout, fetch_order(calls, gateway, id)).await {
                Ok(polled) => polled,
                Err(_) => poll_timed_out().fail(),
            }
        };
        let give_up_at = found + options.give_up_after;
        let quiet_at = answered + options.quiet_after;
        let polled = match watch(poll, &mut interrupt, quiet_at, give_up_at, &mut told).await {
            Step::Done(polled) => polled,
            Step::Interrupted => return interrupted(last),
            Step::GaveUp => return gave_up(last, missed),
        };
        if polled.is_ok() {
            answered = Instant::now();
            told.answered();
        }
        match polled {
            Ok(Some(order)) => {
                found = answered;
                missed = None;
                if order.is_settled() {
                    return Ok(order);
                }
                told.order(order.status, order.detail.as_deref());
                if started.elapsed() > options.timeout {
                    return WaitTimedOutSnafu {
                        order: Box::new(order),
                    }
                    .fail();
                }
                last = Some(order);
            }
            Ok(None) => missed = Some(None),
            Err(error) => missed = Some(Some(error)),
        }

        let give_up_at = found + options.give_up_after;
        let quiet_at = answered + options.quiet_after;
        let next = sleep_until(Instant::now() + options.poll_interval);
        match watch(next, &mut interrupt, quiet_at, give_up_at, &mut told).await {
            Step::Done(()) => {}
            Step::Interrupted => return interrupted(last),
            Step::GaveUp => return gave_up(last, missed),
        }
    }
}

/// Where to buy instead, said whenever CyclePay's own gateway cannot be
/// reached on the network chosen.
pub const MAINNET_HINT: &str = "CyclePay sells cycles on mainnet: use -n ic";

/// Refuse a principal that cannot be a gateway's canister id, before any
/// network is asked about it: the management canister, the anonymous
/// principal, and any principal of a kind no canister has (an identity's,
/// say). A canister's id is an opaque principal, whose last byte is 1.
pub fn check_gateway_id(gateway: Principal) -> Result<(), CyclesPurchaseError> {
    let what = match gateway.as_slice() {
        [] => "the management canister",
        _ if gateway == Principal::anonymous() => "the anonymous principal",
        [.., 0x01] => return Ok(()),
        _ => "not a canister id",
    };
    NotACanisterSnafu { gateway, what }.fail()
}

/// Refuse to buy through CyclePay's own gateway anywhere but mainnet, told
/// apart by `root_key`, the key the agent verifies the network's answers
/// with. Any other gateway could be on any network, and is left to say for
/// itself whether it is there.
///
/// Checked before the gateway is asked anything, so the answer does not
/// depend on how a network reports a canister it does not have, and the
/// same thing is said however the network was chosen.
pub fn check_network(gateway: Principal, root_key: &[u8]) -> Result<(), CyclesPurchaseError> {
    if is_cyclepay(gateway) && !is_mainnet(root_key) {
        return GatewayNotOnThisNetworkSnafu { gateway }.fail();
    }
    Ok(())
}

/// Whether the network whose answers verify against `root_key` is mainnet:
/// no other network can sign with mainnet's key.
pub fn is_mainnet(root_key: &[u8]) -> bool {
    root_key == IC_ROOT_KEY.as_slice()
}

/// A call saying the gateway canister does not exist becomes
/// [`CyclesPurchaseError::GatewayNotFound`]; anything else goes to `wrap`.
/// Either a replica says so (IC0301), or a boundary node does, before any
/// replica sees the call, for an id it cannot route.
fn map_absent(
    source: TypedCallError,
    gateway: Principal,
    wrap: impl FnOnce(TypedCallError) -> CyclesPurchaseError,
) -> CyclesPurchaseError {
    match &source {
        TypedCallError::Call { source: call }
            if call.is_canister_not_found() || is_unroutable(call) =>
        {
            CyclesPurchaseError::GatewayNotFound { gateway }
        }
        _ => wrap(source),
    }
}

/// The replica's error code for a call to a method the canister does not
/// export (`CanisterMethodNotFound`).
const METHOD_NOT_FOUND: &str = "IC0536";

/// Whether `call` reached a canister that exports no such method. A replica
/// that gave no code leaves only its words to go on.
fn is_method_not_found(call: &CallError) -> bool {
    match call.code() {
        Some(code) => code == METHOD_NOT_FOUND,
        None => call
            .message()
            .is_some_and(|message| message.contains("has no query method")),
    }
}

/// Whether a boundary node answered `call` that the canister it was addressed
/// to does not exist: an HTTP 4xx whose body is its `canister_not_found`.
/// The agent reports that as a failure, not a rejection, since no replica
/// gave a verdict.
fn is_unroutable(call: &CallError) -> bool {
    let CallError::Failed { source, .. } = call else {
        return false;
    };
    matches!(
        source.downcast_ref::<AgentError>(),
        Some(AgentError::HttpError(payload))
            if (400..500).contains(&payload.status)
                && String::from_utf8_lossy(&payload.content).contains("canister_not_found")
    )
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
