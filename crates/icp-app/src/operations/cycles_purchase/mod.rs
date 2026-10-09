//! Buying cycles with a card through a cycles gateway canister.
//!
//! The buyer is whoever makes the calls: the order is created as the caller,
//! the cycles go to the caller's own default cycles-ledger account, and only
//! the caller can read the order back. The flow is quote, create, pay on the
//! hosted checkout page the order carries, then wait for the gateway to flip
//! the order to `delivered`.
//!
//! Nothing here prints. [`wait_for_order`] reports its progress as
//! [`icp_events`] tasks, one per [`PurchasePhase`], and hands the last order
//! it saw back inside its error when it is interrupted, so the caller can say
//! how to resume.

mod money;

#[cfg(test)]
mod tests;

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use candid::{Nat, Principal};
use icp_canister_interfaces::cycles_gateway::{
    Account, Amount, CANCEL_ORDER_METHOD, CREATE_ORDER_METHOD, CYCLES_GATEWAY_CANISTER_ID_ENV,
    CancelOrderError, CancelOrderResult, CreateOrderError, CreateOrderResult, Destination,
    ExpiredBy, GET_ORDER_METHOD, LIST_ORDERS_METHOD, ListOrdersResult, Order, OrderStatus,
    QUOTE_PREVIEWS_METHOD, QuotePreviews, Reason,
};
use icp_events::TaskOutcome;
use icp_project::calls::{CanisterCalls, RouteTo, TypedCallError, query_typed, update_typed};
use icp_project::operations::task::{PurchasePhase, Reporter, Task, TaskReporter};
use num_traits::ToPrimitive;
use snafu::{OptionExt, Snafu};
use tokio::{select, time::sleep};

pub use money::{Currency, Money, ParseMoneyError};

/// How often [`wait_for_order`] asks the gateway for the order.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// How far the rate may move against the buyer between quote and creation
/// before the gateway refuses the order instead of delivering less.
pub const QUOTE_SLIPPAGE_PERCENT: u128 = 5;

/// Orders per `list_orders` page when looking for the caller's open order.
const LIST_ORDERS_PAGE: u8 = 50;

/// Pages of `list_orders` to walk before giving up on finding an open order.
const LIST_ORDERS_MAX_PAGES: usize = 3;

#[derive(Debug, Snafu)]
pub enum CyclesPurchaseError {
    #[snafu(display(
        "the cycles gateway canister {gateway} does not exist on this network. \
         The default gateway is deployed on mainnet (`-n ic`); \
         --gateway or {CYCLES_GATEWAY_CANISTER_ID_ENV} chooses another"
    ))]
    GatewayNotFound { gateway: Principal },

    #[snafu(display("failed to get a quote from cycles gateway {gateway}"))]
    QuotePreviews {
        gateway: Principal,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display("cycles gateway {gateway} returned no quote for {amount}"))]
    QuoteMissing { gateway: Principal, amount: Money },

    #[snafu(display(
        "cycles gateway {gateway} is not pricing orders right now (its quote carries no \
         cycles figure); try again in a few minutes"
    ))]
    RateUnavailable { gateway: Principal },

    #[snafu(display(
        "cycles gateway {gateway} quotes no amount that buys {cycles} cycles; \
         try a different amount, or spend a sum with --amount"
    ))]
    CyclesUnpriceable { gateway: Principal, cycles: u128 },

    #[snafu(display("failed to create an order on cycles gateway {gateway}"))]
    CreateOrder {
        gateway: Principal,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display("the cycles gateway refused the order: {error}"))]
    OrderRefused { error: CreateOrderError },

    #[snafu(display(
        "you already have an open order {} ({open} of {max} allowed)", order.id
    ))]
    OpenOrderExists {
        order: Box<OrderView>,
        open: u128,
        max: u128,
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
         an order is visible only to the identity that created it"
    ))]
    OrderNotFound {
        gateway: Principal,
        id: String,
        caller: Principal,
    },

    #[snafu(display("failed to list orders on cycles gateway {gateway}"))]
    ListOrders {
        gateway: Principal,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display("failed to cancel order {id} on cycles gateway {gateway}"))]
    CancelOrder {
        gateway: Principal,
        id: String,
        #[snafu(source(from(TypedCallError, Box::new)))]
        source: Box<TypedCallError>,
    },

    #[snafu(display("the cycles gateway refused to cancel order {id}: {error}"))]
    CancelRefused { id: String, error: CancelOrderError },

    #[snafu(display("the cycles gateway returned a value for '{field}' that does not fit"))]
    ConvertField { field: &'static str },

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

/// What an amount buys, as the gateway priced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    /// What the card is charged.
    pub amount: Money,
    /// The card-processing fee included in `amount`.
    pub fee: Money,
    /// The cycles delivered for `amount` at the current rate.
    pub cycles: u128,
    /// The fewest cycles the buyer will accept. Passed to the gateway so a
    /// rate move against the buyer is refused rather than silently
    /// delivering less: the quote less [`QUOTE_SLIPPAGE_PERCENT`] when
    /// spending a sum, or the cycles asked for when buying a number of them.
    pub minimum_cycles: u128,
}

impl Quote {
    /// Whether a fresh quote for the same target is still within
    /// [`QUOTE_SLIPPAGE_PERCENT`] of this one: at least the cycles this one
    /// would accept, for no more than this one's amount plus the slippage.
    /// A buyer who was shown this quote and is not asked again can be held
    /// to that much, and no more.
    pub fn tolerates(&self, fresh: &Quote) -> bool {
        fresh.cycles >= self.minimum_cycles
            && fresh.amount.currency == self.amount.currency
            && fresh.amount.minor_units <= with_slippage_up(self.amount.minor_units)
    }
}

/// The fewest cycles a buyer who is spending a sum will accept for it.
/// Divides first: the figure is the gateway's, and may be any size.
fn with_slippage(cycles: u128) -> u128 {
    cycles / 100 * (100 - QUOTE_SLIPPAGE_PERCENT)
}

/// The most a buyer who asked for a number of cycles will pay for them.
fn with_slippage_up(minor_units: u64) -> u64 {
    minor_units.saturating_add((minor_units / 100).saturating_mul(QUOTE_SLIPPAGE_PERCENT as u64))
}

/// Price a [`Target`].
pub async fn quote_target(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    target: Target,
) -> Result<Quote, CyclesPurchaseError> {
    match target {
        Target::Spend(amount) => quote(calls, gateway, amount).await,
        Target::Receive { cycles, currency } => {
            quote_for_cycles(calls, gateway, cycles, currency).await
        }
    }
}

/// An order, converted out of Candid's arbitrary-precision numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderView {
    pub id: String,
    pub status: OrderStatus,
    /// The cycles promised to the buyer, fixed at creation.
    pub locked_cycles: u128,
    /// The hosted checkout page, while the order is payable.
    pub checkout_url: Option<String>,
    /// When the checkout page stops accepting payment, in nanoseconds since
    /// the Unix epoch.
    pub expires_at_ns: Option<i64>,
    /// What the card was charged, once paid.
    pub paid: Option<Money>,
    pub expired_by: Option<ExpiredBy>,
    pub abandoned_reason: Option<String>,
}

impl TryFrom<Order> for OrderView {
    type Error = CyclesPurchaseError;

    fn try_from(order: Order) -> Result<Self, Self::Error> {
        Ok(Self {
            id: order.id,
            status: order.status,
            locked_cycles: order.locked_cycles.0.to_u128().context(ConvertFieldSnafu {
                field: "lockedCycles",
            })?,
            checkout_url: order.stripe_session_url,
            expires_at_ns: order
                .expires_at_ns
                .map(|ns| {
                    ns.0.to_i64().context(ConvertFieldSnafu {
                        field: "expiresAtNs",
                    })
                })
                .transpose()?,
            paid: order
                .paid_usd_cents
                .map(|cents| {
                    Money::from_nat_minor_units(&cents, Currency::Usd).context(ConvertFieldSnafu {
                        field: "paidUsdCents",
                    })
                })
                .transpose()?,
            expired_by: order.expired_by,
            abandoned_reason: order.abandoned_reason,
        })
    }
}

/// Ask the gateway what `amount` buys right now.
///
/// The figure comes from the same function that prices an order, so it is
/// displayed as given rather than recomputed.
pub async fn quote(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    amount: Money,
) -> Result<Quote, CyclesPurchaseError> {
    let previews = previews(calls, gateway, &[amount]).await?;
    let preview = previews
        .into_iter()
        .next()
        .context(QuoteMissingSnafu { gateway, amount })?;
    let cycles = preview.cycles.context(RateUnavailableSnafu { gateway })?;
    Ok(Quote {
        amount,
        fee: preview.fee,
        cycles,
        minimum_cycles: with_slippage(cycles),
    })
}

/// A gateway quote with its numbers converted, keyed by the amount quoted.
#[derive(Clone, Copy, Debug)]
struct Preview {
    amount: Money,
    fee: Money,
    /// What `amount` buys; `None` when the gateway has no rate, or the fee
    /// swallows the whole amount.
    cycles: Option<u128>,
    /// `amount` less `fee`; `None` when the fee swallows it.
    net: Option<Money>,
}

/// Quote several amounts in one round trip.
async fn previews(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    amounts: &[Money],
) -> Result<Vec<Preview>, CyclesPurchaseError> {
    let minor_units: Vec<Nat> = amounts
        .iter()
        .map(|amount| match amount.currency {
            Currency::Usd => Nat::from(amount.minor_units),
        })
        .collect();
    let (previews,): (QuotePreviews,) =
        query_typed(calls, gateway, QUOTE_PREVIEWS_METHOD, (minor_units,))
            .await
            .map_err(|source| {
                map_absent(source, gateway, |source| {
                    CyclesPurchaseError::QuotePreviews {
                        gateway,
                        source: Box::new(source),
                    }
                })
            })?;
    previews
        .quotes
        .into_iter()
        .zip(amounts)
        .map(|(preview, amount)| {
            let currency = amount.currency;
            Ok(Preview {
                amount: Money::from_nat_minor_units(&preview.usd_cents, currency)
                    .context(ConvertFieldSnafu { field: "usdCents" })?,
                fee: Money::from_nat_minor_units(&preview.fee_cents, currency)
                    .context(ConvertFieldSnafu { field: "feeCents" })?,
                cycles: preview
                    .cycles
                    .map(|cycles| {
                        cycles
                            .0
                            .to_u128()
                            .context(ConvertFieldSnafu { field: "cycles" })
                    })
                    .transpose()?,
                net: preview
                    .net_cents
                    .map(|net| {
                        Money::from_nat_minor_units(&net, currency)
                            .context(ConvertFieldSnafu { field: "netCents" })
                    })
                    .transpose()?,
            })
        })
        .collect()
}

/// Amounts whose quotes anchor the search in [`quote_for_cycles`]: large
/// enough that the fee does not swallow them, far enough apart that the
/// fee's variable part shows between them.
const PROBE_MINOR_UNITS: [u64; 2] = [1_000, 2_000];

/// Rounds of estimate-and-check before giving up on a cycles target.
const CYCLES_SEARCH_ROUNDS: usize = 4;

/// Find the least `currency` whose quote delivers at least `cycles`.
///
/// The gateway prices an amount as `(amount - fee) * rate` with a fee of a
/// fixed part plus a share of the amount, but tells only the outcome, so two
/// anchor quotes are read back into that model, the model names the amount
/// that should just clear the target, and that amount and its neighbours
/// are quoted for real; the least that clears wins. If rounding leaves all
/// of them short, the shortfall re-anchors the model and it tries again.
pub async fn quote_for_cycles(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    cycles: u128,
    currency: Currency,
) -> Result<Quote, CyclesPurchaseError> {
    let unpriceable = || CyclesUnpriceableSnafu { gateway, cycles };
    let probes = PROBE_MINOR_UNITS.map(|units| Money::from_minor_units(units, currency));
    let quoted = previews(calls, gateway, &probes).await?;
    let [mut low, mut high]: [Preview; 2] = quoted.try_into().ok().context(QuoteMissingSnafu {
        gateway,
        amount: probes[1],
    })?;
    if high.cycles.is_none() {
        return RateUnavailableSnafu { gateway }.fail();
    }

    for _ in 0..CYCLES_SEARCH_ROUNDS {
        let estimate = estimate_amount(cycles, &low, &high).context(unpriceable())?;
        let candidates: Vec<Money> = (estimate.saturating_sub(1)..=estimate.saturating_add(2))
            .filter(|units| *units > 0)
            .map(|units| Money::from_minor_units(units, currency))
            .collect();
        let quoted = previews(calls, gateway, &candidates).await?;
        if let Some(hit) = quoted
            .iter()
            .filter(|preview| preview.cycles.is_some_and(|got| got >= cycles))
            .min_by_key(|preview| preview.amount.minor_units)
        {
            return Ok(Quote {
                amount: hit.amount,
                fee: hit.fee,
                cycles: hit.cycles.unwrap_or(cycles),
                minimum_cycles: cycles,
            });
        }
        // Every candidate fell short: re-anchor on the two largest real
        // quotes and let the model correct itself.
        let mut priced: Vec<Preview> = quoted
            .into_iter()
            .filter(|preview| preview.cycles.is_some())
            .collect();
        priced.sort_by_key(|preview| preview.amount.minor_units);
        match priced.as_slice() {
            [.., a, b] => {
                low = *a;
                high = *b;
            }
            _ => return unpriceable().fail(),
        }
    }
    unpriceable().fail()
}

/// The amount that should deliver `cycles`, read off two priced quotes:
/// the rate is cycles per net minor unit on `high`, the fee's variable
/// share is the fee difference over the amount difference, and the fixed
/// part is what remains. `None` when the figures do not fit or do not
/// describe a price.
fn estimate_amount(cycles: u128, low: &Preview, high: &Preview) -> Option<u64> {
    let rate_cycles = high.cycles?;
    let rate_net = u128::from(high.net?.minor_units);
    if rate_cycles == 0 || rate_net == 0 {
        return None;
    }
    let net_needed = cycles.checked_mul(rate_net)?.div_ceil(rate_cycles).max(1);

    let amount_delta = u128::from(high.amount.minor_units)
        .checked_sub(u128::from(low.amount.minor_units))
        .filter(|delta| *delta > 0)?;
    let fee_delta = u128::from(high.fee.minor_units)
        .saturating_sub(u128::from(low.fee.minor_units))
        .min(amount_delta - 1);
    // fixed = fee(low) - share * amount(low), kept over `amount_delta`.
    let fixed_scaled = (u128::from(low.fee.minor_units) * amount_delta)
        .saturating_sub(fee_delta * u128::from(low.amount.minor_units));
    // amount = (net + fixed) / (1 - share)
    let amount = net_needed
        .checked_mul(amount_delta)?
        .checked_add(fixed_scaled)?
        .div_ceil(amount_delta - fee_delta);
    u64::try_from(amount).ok()
}

/// Create an order for `quote`, delivering to the caller's own default
/// cycles-ledger account and refusing fewer cycles than
/// [`Quote::minimum_cycles`].
///
/// When the gateway refuses because the caller already has an open order,
/// that order is looked up and returned in
/// [`CyclesPurchaseError::OpenOrderExists`] so the caller can resume or cancel
/// it instead of trying again.
pub async fn place_order(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    quote: &Quote,
) -> Result<OrderView, CyclesPurchaseError> {
    let amount = match quote.amount.currency {
        Currency::Usd => Amount::Custom(Nat::from(quote.amount.minor_units)),
    };
    let destination = Destination::CyclesLedgerAccount(Account {
        owner: calls.caller(),
        subaccount: None,
    });
    let min_cycles = Some(Nat::from(quote.minimum_cycles));

    let (result,): (CreateOrderResult,) = update_typed(
        calls,
        gateway,
        CREATE_ORDER_METHOD,
        (amount, destination, min_cycles),
        RouteTo::Callee,
        0,
    )
    .await
    .map_err(|source| {
        map_absent(source, gateway, |source| CyclesPurchaseError::CreateOrder {
            gateway,
            source: Box::new(source),
        })
    })?;

    match result {
        CreateOrderResult::Ok(created) => created.order.try_into(),
        CreateOrderResult::Err(CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders {
            max,
            open,
        })) => match find_open_order(calls, gateway).await? {
            Some(order) => OpenOrderExistsSnafu {
                order: Box::new(order),
                open: open.0.to_u128().unwrap_or(u128::MAX),
                max: max.0.to_u128().unwrap_or(u128::MAX),
            }
            .fail(),
            None => OrderRefusedSnafu {
                error: CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders { max, open }),
            }
            .fail(),
        },
        CreateOrderResult::Err(error) => OrderRefusedSnafu { error }.fail(),
    }
}

/// Read one of the caller's orders.
///
/// The gateway answers `null` both for an order that does not exist and for
/// one the caller does not own; both come back as
/// [`CyclesPurchaseError::OrderNotFound`].
pub async fn get_order(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    id: &str,
) -> Result<OrderView, CyclesPurchaseError> {
    let (order,): (Option<Order>,) =
        query_typed(calls, gateway, GET_ORDER_METHOD, (id.to_owned(),))
            .await
            .map_err(|source| {
                map_absent(source, gateway, |source| CyclesPurchaseError::GetOrder {
                    gateway,
                    id: id.to_owned(),
                    source: Box::new(source),
                })
            })?;
    order
        .context(OrderNotFoundSnafu {
            gateway,
            id,
            caller: calls.caller(),
        })?
        .try_into()
}

/// The caller's open order — created or paid — if there is one.
pub async fn find_open_order(
    calls: &dyn CanisterCalls,
    gateway: Principal,
) -> Result<Option<OrderView>, CyclesPurchaseError> {
    let mut cursor: Option<String> = None;
    for _ in 0..LIST_ORDERS_MAX_PAGES {
        let (page,): (ListOrdersResult,) = query_typed(
            calls,
            gateway,
            LIST_ORDERS_METHOD,
            (cursor.clone(), Nat::from(LIST_ORDERS_PAGE)),
        )
        .await
        .map_err(|source| {
            map_absent(source, gateway, |source| CyclesPurchaseError::ListOrders {
                gateway,
                source: Box::new(source),
            })
        })?;
        if let Some(order) = page
            .orders
            .into_iter()
            .find(|order| !order.status.is_terminal())
        {
            return order.try_into().map(Some);
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(None)
}

/// Cancel one of the caller's open orders.
pub async fn cancel_order(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    id: &str,
) -> Result<OrderView, CyclesPurchaseError> {
    let (result,): (CancelOrderResult,) = update_typed(
        calls,
        gateway,
        CANCEL_ORDER_METHOD,
        (id.to_owned(),),
        RouteTo::Callee,
        0,
    )
    .await
    .map_err(|source| {
        map_absent(source, gateway, |source| CyclesPurchaseError::CancelOrder {
            gateway,
            id: id.to_owned(),
            source: Box::new(source),
        })
    })?;
    match result {
        CancelOrderResult::Ok(order) => (*order).try_into(),
        CancelOrderResult::Err(error) => CancelRefusedSnafu { id, error }.fail(),
    }
}

/// Bounds on [`wait_for_order`].
#[derive(Clone, Debug)]
pub struct WaitOptions {
    /// Time between polls.
    pub poll_interval: Duration,
    /// How long past its own deadline a still-payable order is waited on
    /// before giving up. The gateway expires it on a timer, not instantly.
    pub expiry_grace: Duration,
    /// How long a payable order without a deadline is waited on.
    pub max_without_expiry: Duration,
    /// How long a paid order is given to be delivered.
    pub max_paid_wait: Duration,
    /// Consecutive unanswered polls tolerated before giving up.
    pub max_consecutive_transient: u32,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            expiry_grace: Duration::from_secs(2 * 60),
            max_without_expiry: Duration::from_secs(30 * 60),
            max_paid_wait: Duration::from_secs(15 * 60),
            max_consecutive_transient: 20,
        }
    }
}

/// The legs of a wait, reported as one [`Task`] each so a consumer can tick
/// off "payment received" before "delivered".
struct Phases<'a> {
    reporter: &'a Reporter,
    id: &'a str,
    current: Option<(PurchasePhase, TaskReporter)>,
}

impl Phases<'_> {
    fn of(status: OrderStatus) -> PurchasePhase {
        match status {
            OrderStatus::Paid => PurchasePhase::Delivering,
            _ => PurchasePhase::AwaitingPayment,
        }
    }

    /// Move to the phase `status` is in, finishing the one before it.
    fn enter(&mut self, status: OrderStatus) {
        let phase = Self::of(status);
        if self.current.as_ref().is_some_and(|(p, _)| *p == phase) {
            return;
        }
        self.finish(TaskOutcome::succeeded());
        let task = self.reporter.task(Task::cycles_purchase(self.id, phase));
        self.current = Some((phase, task));
    }

    fn finish(&mut self, outcome: TaskOutcome) {
        if let Some((_, task)) = self.current.take() {
            task.finish(outcome);
        }
    }
}

/// Poll the gateway until the order reaches a terminal status.
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
/// once. A terminal status is a success here whatever it is — expired is an
/// answer — and the caller decides what each one means.
pub async fn wait_for_order(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    id: &str,
    initial: Option<OrderView>,
    options: &WaitOptions,
    interrupt: impl Future<Output = ()>,
    reporter: &Reporter,
) -> Result<OrderView, CyclesPurchaseError> {
    let mut interrupt = pin!(interrupt);
    let mut phases = Phases {
        reporter,
        id,
        current: None,
    };
    phases.enter(initial.as_ref().map_or(OrderStatus::Created, |o| o.status));
    let mut last = initial;
    let mut transient_streak = 0u32;
    let started = Instant::now();
    let mut paid_since: Option<Instant> = None;

    let interrupted = |last: Option<OrderView>| {
        let order = last.unwrap_or_else(|| OrderView {
            id: id.to_owned(),
            status: OrderStatus::Created,
            locked_cycles: 0,
            checkout_url: None,
            expires_at_ns: None,
            paid: None,
            expired_by: None,
            abandoned_reason: None,
        });
        InterruptedSnafu {
            order: Box::new(order),
        }
        .fail()
    };

    loop {
        let polled = select! {
            polled = get_order(calls, gateway, id) => polled,
            _ = &mut interrupt => {
                phases.finish(TaskOutcome::failed("interrupted"));
                return interrupted(last);
            }
        };
        match polled {
            Ok(order) => {
                transient_streak = 0;
                if order.status.is_terminal() {
                    phases.finish(match order.status {
                        OrderStatus::Delivered => TaskOutcome::succeeded(),
                        status => TaskOutcome::failed(format!("order {status}")),
                    });
                    return Ok(order);
                }
                phases.enter(order.status);
                if order.status == OrderStatus::Paid && paid_since.is_none() {
                    paid_since = Some(Instant::now());
                }
                let overdue = match (order.status, order.expires_at_ns) {
                    (OrderStatus::Paid, _) => {
                        paid_since.is_some_and(|since| since.elapsed() > options.max_paid_wait)
                    }
                    (_, Some(expires_at_ns)) => {
                        now_ns() > expires_at_ns.saturating_add(as_ns(options.expiry_grace))
                    }
                    (_, None) => started.elapsed() > options.max_without_expiry,
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
            Err(CyclesPurchaseError::GetOrder { source, .. }) if is_transient(&source) => {
                transient_streak += 1;
                if transient_streak > options.max_consecutive_transient {
                    phases.finish(TaskOutcome::failed("lost contact with the gateway"));
                    return Err(CyclesPurchaseError::WaitUnanswered {
                        gateway,
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
                phases.finish(TaskOutcome::failed("interrupted"));
                return interrupted(last);
            }
        }
    }
}

/// A rejection saying the gateway canister does not exist becomes
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

fn is_transient(error: &TypedCallError) -> bool {
    matches!(error, TypedCallError::Call { source } if source.is_transient())
}

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn as_ns(duration: Duration) -> i64 {
    duration.as_nanos().try_into().unwrap_or(i64::MAX)
}
