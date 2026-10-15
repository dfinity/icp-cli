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
//! [`PurchasePhase`] the order enters, and hands the last order it saw back
//! inside its error when it is interrupted, so the caller can say where it
//! stands.

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, Instant};

use candid::Principal;
use icp_project::calls::{CanisterCalls, RouteTo, TypedCallError, query_typed, update_typed};
use icp_project::prelude::IC_ROOT_KEY;
use num_traits::ToPrimitive;
use snafu::{OptionExt, Snafu};
use std::fmt;
use tokio::{select, time::sleep};
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
        "gave up waiting for order {} (last status: {})", order.id, order.status_text()
    ))]
    WaitTimedOut { order: Box<OrderView> },
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
            Self::PaymentPending => "paid, payment pending",
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
    /// Consecutive polls that said nothing of the order (unanswered,
    /// refused, or finding none) before the wait gives up: five minutes of
    /// them at [`POLL_INTERVAL`], longer than CyclePay's backend is stopped
    /// for during an upgrade.
    pub max_consecutive_misses: u32,
    /// How long the order is waited on in one run. Its deadline is a day
    /// away; a buyer who has not paid by now is told where it stands.
    pub timeout: Duration,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            max_consecutive_misses: 100,
            timeout: Duration::from_secs(60 * 60),
        }
    }
}

/// Which leg of a card purchase of cycles is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchasePhase {
    /// The order is payable and the buyer has not paid yet.
    AwaitingPayment,
    /// The card was charged; the gateway is sending the cycles.
    Delivering,
}

/// The legs of a wait, each told to `on_phase` as the order enters it.
struct Phases<F> {
    current: Option<PurchasePhase>,
    on_phase: F,
}

impl<F: FnMut(PurchasePhase)> Phases<F> {
    fn of(status: Option<CardOrderStatus>) -> PurchasePhase {
        match status {
            Some(CardOrderStatus::PaymentPending | CardOrderStatus::Processing) => {
                PurchasePhase::Delivering
            }
            _ => PurchasePhase::AwaitingPayment,
        }
    }

    /// Move to the phase `status` is in. A state this CLI cannot place
    /// leaves the phase as it is.
    fn enter(&mut self, status: Option<CardOrderStatus>) {
        if status.is_none() && self.current.is_some() {
            return;
        }
        let phase = Self::of(status);
        if self.current != Some(phase) {
            self.current = Some(phase);
            (self.on_phase)(phase);
        }
    }
}

/// Poll the backend until the order reaches a terminal status.
///
/// `initial` is the order as last seen, if the caller has it: it names the
/// phase the wait starts in, and it is what
/// [`CyclesPurchaseError::Interrupted`] carries if `interrupt` resolves before
/// the first poll answers. `on_phase` is told each [`PurchasePhase`] the
/// order enters, the first before the first poll.
///
/// `interrupt` is raced against every poll as well as the sleep between
/// them, so an interrupt lands even while a call is stalled.
///
/// A poll that says nothing of the order is tried again, up to
/// [`WaitOptions::max_consecutive_misses`] in a row: one the network never
/// answered, one the backend refused (as it does for a few minutes during
/// an upgrade), or one answered `null` (a replica that has not caught up
/// with a new order, or an unpaid order forgotten once it expired). A
/// terminal status is a success here whatever it is — a refund is an answer
/// — and the caller decides what each one means.
pub async fn wait_for_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    id: &str,
    initial: Option<OrderView>,
    options: &WaitOptions,
    interrupt: impl Future<Output = ()>,
    on_phase: impl FnMut(PurchasePhase),
) -> Result<OrderView, CyclesPurchaseError> {
    // An order already settled has nothing to wait for: no phase to show,
    // and no call that could turn a known end into an error.
    if let Some(order) = initial.as_ref().filter(|order| order.is_settled()) {
        return Ok(order.clone());
    }
    let mut interrupt = pin!(interrupt);
    let mut phases = Phases {
        current: None,
        on_phase,
    };
    phases.enter(
        initial
            .as_ref()
            .map_or(Some(CardOrderStatus::AwaitingPayment), |o| o.status),
    );
    let mut last = initial;
    let mut misses = 0u32;
    let started = Instant::now();

    let interrupted = |last: Option<OrderView>| {
        let order = last.unwrap_or_else(|| OrderView {
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
        });
        InterruptedSnafu {
            order: Box::new(order),
        }
        .fail()
    };

    loop {
        let polled = select! {
            polled = fetch_order(calls, gateway, id) => polled,
            _ = &mut interrupt => return interrupted(last),
        };
        match polled {
            Ok(Some(order)) => {
                misses = 0;
                if order.is_settled() {
                    return Ok(order);
                }
                phases.enter(order.status);
                if started.elapsed() > options.timeout {
                    return WaitTimedOutSnafu {
                        order: Box::new(order),
                    }
                    .fail();
                }
                last = Some(order);
            }
            Ok(None) | Err(_) if misses < options.max_consecutive_misses => misses += 1,
            Ok(None) => {
                return OrderNotFoundSnafu {
                    gateway: gateway.canister,
                    id,
                    caller: calls.caller(),
                }
                .fail();
            }
            Err(error) => return Err(error),
        }

        select! {
            _ = sleep(options.poll_interval) => {}
            _ = &mut interrupt => return interrupted(last),
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

/// A replica's IC0301, saying the gateway canister does not exist, becomes
/// [`CyclesPurchaseError::GatewayNotFound`]; anything else goes to `wrap`.
fn map_absent(
    source: TypedCallError,
    gateway: Principal,
    wrap: impl FnOnce(TypedCallError) -> CyclesPurchaseError,
) -> CyclesPurchaseError {
    match &source {
        TypedCallError::Call { source: call } if call.is_canister_not_found() => {
            CyclesPurchaseError::GatewayNotFound { gateway }
        }
        _ => wrap(source),
    }
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
