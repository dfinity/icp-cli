//! Interface to a **cycles gateway**: a canister that sells cycles for a card
//! payment. The buyer creates an order as itself, pays on the hosted checkout
//! page the order carries, and the gateway transfers the locked cycles to the
//! buyer's own cycles-ledger account once the payment settles.
//!
//! "Gateway" names the role the canister plays. To the CLI it is a canister
//! id: every interaction here is an ordinary query or update call against it.
//! The gateway is a Motoko canister, so field names are camelCase and variant
//! tags are lowercase on the wire; the Rust names below are idiomatic and
//! renamed with `serde`.
//!
//! Only the fields and methods the CLI reads are declared. Candid lets the
//! canister report more in a record than the client names, so `Order` leaves
//! out pricing, problem and audit fields the CLI has no use for.

use std::env::{self, VarError};
use std::fmt;

use candid::{CandidType, Int, Nat, Principal};
use serde::Deserialize;

/// Default cycles gateway canister: the CyclePay backend on mainnet.
///
/// Overridable at runtime via the [`CYCLES_GATEWAY_CANISTER_ID_ENV`]
/// environment variable — see [`cycles_gateway_canister_id`].
pub const CYCLES_GATEWAY_CID: &str = "saz2a-riaaa-aaaay-aadha-cai";

/// Environment variable that overrides [`CYCLES_GATEWAY_CID`].
pub const CYCLES_GATEWAY_CANISTER_ID_ENV: &str = "ICP_CYCLES_GATEWAY_CANISTER_ID";

/// Public query pricing a list of amounts: `(vec nat) -> (QuotePreviews)`.
/// Amounts are USD cents. Computed by the same function that prices an order.
pub const QUOTE_PREVIEWS_METHOD: &str = "quote_previews";

/// Public query pricing a list of cycle targets: `(vec nat) -> (CyclesQuotes)`.
/// For each target, the least amount (USD cents) whose quote delivers at
/// least that many cycles, run through the same function that prices an
/// order, so `create_order` for that amount with the target as `minCycles`
/// is admitted at the figures quoted.
pub const QUOTE_FOR_CYCLES_METHOD: &str = "quote_for_cycles";

/// Update creating an order for the caller:
/// `(Amount, Destination, opt nat) -> (CreateOrderResult)`.
pub const CREATE_ORDER_METHOD: &str = "create_order";

/// Owner-scoped query: `(text) -> (opt Order)`. `null` for an order the
/// caller does not own, as well as for one that does not exist.
pub const GET_ORDER_METHOD: &str = "get_order";

/// Update cancelling one of the caller's open orders:
/// `(text) -> (CancelOrderResult)`.
pub const CANCEL_ORDER_METHOD: &str = "cancel_order";

/// Caller-scoped, paginated query over the caller's own orders:
/// `(opt text, nat) -> (ListOrdersResult)`.
pub const LIST_ORDERS_METHOD: &str = "list_orders";

/// Resolve the cycles gateway canister id to talk to.
///
/// Uses the value of the [`CYCLES_GATEWAY_CANISTER_ID_ENV`] variable when set
/// and non-empty, otherwise falls back to [`CYCLES_GATEWAY_CID`]. Returns an
/// error when the override is set but invalid — either not a valid principal,
/// or not valid Unicode. Only an absent or empty (whitespace-only) variable
/// uses the default: a configured-but-invalid value must never silently route
/// a payment to the built-in gateway.
pub fn cycles_gateway_canister_id() -> Result<Principal, String> {
    resolve_cycles_gateway_canister_id(env::var(CYCLES_GATEWAY_CANISTER_ID_ENV))
}

/// [`cycles_gateway_canister_id`] with the environment read out, so the
/// precedence can be tested without touching the process environment.
fn resolve_cycles_gateway_canister_id(env: Result<String, VarError>) -> Result<Principal, String> {
    match env {
        Ok(value) if !value.trim().is_empty() => Principal::from_text(value.trim())
            .map_err(|e| format!("invalid {CYCLES_GATEWAY_CANISTER_ID_ENV}: {e}")),
        Err(VarError::NotUnicode(_)) => Err(format!(
            "invalid {CYCLES_GATEWAY_CANISTER_ID_ENV}: not valid Unicode"
        )),
        Ok(_) | Err(VarError::NotPresent) => Ok(Principal::from_text(CYCLES_GATEWAY_CID)
            .expect("CYCLES_GATEWAY_CID is a valid principal")),
    }
}

/// An order's id, as the gateway mints it.
pub type OrderId = String;

/// An ICRC-1 account on the cycles ledger.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct Account {
    pub owner: Principal,
    pub subaccount: Option<Vec<u8>>,
}

/// Where the gateway delivers the cycles. The gateway refuses any account the
/// caller does not own.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum Destination {
    #[serde(rename = "cyclesLedgerAccount")]
    CyclesLedgerAccount(Account),
}

/// What the buyer pays: a custom amount in USD cents, or a preset tier by id.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum Amount {
    #[serde(rename = "custom")]
    Custom(Nat),
    #[serde(rename = "tier")]
    Tier(String),
}

/// An order's lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum OrderStatus {
    /// Created and payable until its checkout session's deadline.
    #[serde(rename = "created")]
    Created,
    /// Paid; the cycles transfer is pending.
    #[serde(rename = "paid")]
    Paid,
    /// The cycles reached the destination account.
    #[serde(rename = "delivered")]
    Delivered,
    /// The checkout session ended without a payment.
    #[serde(rename = "expired")]
    Expired,
    /// Cancelled by its owner.
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Paid, but delivery could not complete; an operator must resolve it.
    #[serde(rename = "needsReview")]
    NeedsReview,
    /// Given up on by an operator.
    #[serde(rename = "abandoned")]
    Abandoned,
}

impl OrderStatus {
    /// Whether the order can still change. Only a created or paid order can.
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Created | Self::Paid)
    }

    /// The status as the gateway spells it on the wire, e.g. `needsReview`.
    /// For output meant to be parsed; [`fmt::Display`] is for people.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Paid => "paid",
            Self::Delivered => "delivered",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::NeedsReview => "needsReview",
            Self::Abandoned => "abandoned",
        }
    }
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Created => "created",
            Self::Paid => "paid",
            Self::Delivered => "delivered",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::NeedsReview => "needs review",
            Self::Abandoned => "abandoned",
        })
    }
}

/// Why an order expired.
#[derive(Clone, Copy, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum ExpiredBy {
    /// The checkout session reached its deadline unpaid.
    #[serde(rename = "sessionExpired")]
    SessionExpired,
    /// The checkout session failed.
    #[serde(rename = "sessionFailed")]
    SessionFailed,
}

impl fmt::Display for ExpiredBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SessionExpired => "the checkout session expired unpaid",
            Self::SessionFailed => "the checkout session failed",
        })
    }
}

/// An order, as the gateway reports it. Only the fields the CLI reads.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub status: OrderStatus,
    /// The cycles promised to the buyer, fixed at creation.
    #[serde(rename = "lockedCycles")]
    pub locked_cycles: Nat,
    /// The hosted checkout page. Set while the order is payable.
    #[serde(rename = "stripeSessionUrl")]
    pub stripe_session_url: Option<String>,
    /// When the checkout session stops accepting payment, in nanoseconds
    /// since the Unix epoch.
    #[serde(rename = "expiresAtNs")]
    pub expires_at_ns: Option<Int>,
    /// What the card was charged, in USD cents, once paid.
    #[serde(rename = "paidUsdCents")]
    pub paid_usd_cents: Option<Nat>,
    #[serde(rename = "expiredBy")]
    pub expired_by: Option<ExpiredBy>,
    #[serde(rename = "abandonedReason")]
    pub abandoned_reason: Option<String>,
    // `destination` is deliberately not read back: it is a variant, and a
    // case this CLI does not know would stop every order decoding.
}

/// The price of one amount, as [`QUOTE_PREVIEWS_METHOD`] reports it.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct QuotePreview {
    /// The cycles this amount buys. `None` when the gateway has no rate.
    pub cycles: Option<Nat>,
    /// Card-processing fee, in USD cents.
    #[serde(rename = "feeCents")]
    pub fee_cents: Nat,
    /// The amount net of fees, in USD cents. `None` when fees exceed it.
    #[serde(rename = "netCents")]
    pub net_cents: Option<Nat>,
    /// The amount quoted, in USD cents.
    #[serde(rename = "usdCents")]
    pub usd_cents: Nat,
}

/// Result of [`QUOTE_PREVIEWS_METHOD`]: one preview per amount asked for.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct QuotePreviews {
    pub quotes: Vec<QuotePreview>,
}

/// Why no amount delivers a number of cycles.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum Unpriceable {
    /// In simulation mode the cycles are scaled down before delivery, and
    /// the scaled figure would not cover the cycles-ledger fee.
    #[serde(rename = "simulationScale")]
    SimulationScale {
        #[serde(rename = "ledgerFee")]
        ledger_fee: Nat,
        #[serde(rename = "scaledCycles")]
        scaled_cycles: Nat,
    },
    /// The card fee leaves nothing of any amount that would buy them.
    #[serde(rename = "stripeFee")]
    StripeFee,
}

impl fmt::Display for Unpriceable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SimulationScale {
                ledger_fee,
                scaled_cycles,
            } => write!(
                f,
                "the gateway is in simulation mode and would deliver {scaled_cycles} cycles, \
                 which does not cover the ledger fee of {ledger_fee}"
            ),
            Self::StripeFee => write!(f, "the card fee leaves no amount that buys them"),
        }
    }
}

/// What [`QUOTE_FOR_CYCLES_METHOD`] found for one cycle target.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum CyclesQuoteOutcome {
    /// The least amount that buys the target is above the gateway's maximum.
    #[serde(rename = "amountAboveMax")]
    AmountAboveMax {
        #[serde(rename = "maxUsdCents")]
        max_usd_cents: Nat,
        #[serde(rename = "usdCents")]
        usd_cents: Nat,
    },
    /// The least amount that buys the target is below the gateway's minimum.
    #[serde(rename = "amountBelowMin")]
    AmountBelowMin {
        #[serde(rename = "minUsdCents")]
        min_usd_cents: Nat,
        #[serde(rename = "usdCents")]
        usd_cents: Nat,
    },
    #[serde(rename = "ok")]
    Ok {
        /// What `usd_cents` actually buys: at least the target.
        #[serde(rename = "cyclesQuoted")]
        cycles_quoted: Nat,
        /// Card-processing fee, in USD cents.
        #[serde(rename = "feeCents")]
        fee_cents: Nat,
        /// `usd_cents` net of fees, in USD cents.
        #[serde(rename = "netCents")]
        net_cents: Nat,
        /// The least amount that buys the target, in USD cents.
        #[serde(rename = "usdCents")]
        usd_cents: Nat,
    },
    /// The gateway has no current exchange rate.
    #[serde(rename = "stale")]
    Stale,
    #[serde(rename = "unpriceable")]
    Unpriceable(Unpriceable),
}

/// One answer of [`QUOTE_FOR_CYCLES_METHOD`], keyed by the target asked for.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct CyclesQuote {
    pub cycles: Nat,
    pub outcome: CyclesQuoteOutcome,
}

/// Result of [`QUOTE_FOR_CYCLES_METHOD`]: one answer per target asked for.
/// The `rates` the gateway also reports are deliberately not read back.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct CyclesQuotes {
    pub quotes: Vec<CyclesQuote>,
}

/// Why the gateway would not admit an order.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum Reason {
    #[serde(rename = "amountAboveMax")]
    AmountAboveMax {
        #[serde(rename = "maxUsdCents")]
        max_usd_cents: Nat,
        #[serde(rename = "usdCents")]
        usd_cents: Nat,
    },
    #[serde(rename = "amountBelowMin")]
    AmountBelowMin {
        #[serde(rename = "minUsdCents")]
        min_usd_cents: Nat,
        #[serde(rename = "usdCents")]
        usd_cents: Nat,
    },
    /// The gateway sells only to allow-listed principals (simulation mode).
    #[serde(rename = "buyerNotAllowed")]
    BuyerNotAllowed,
    #[serde(rename = "canisterCyclesLow")]
    CanisterCyclesLow { balance: Nat, min: Nat },
    #[serde(rename = "reserveShort")]
    ReserveShort { available: Nat, requested: Nat },
    #[serde(rename = "tooManyOpenOrders")]
    TooManyOpenOrders { max: Nat, open: Nat },
    #[serde(rename = "unboundedGiveaway")]
    UnboundedGiveaway {
        #[serde(rename = "reserveFloor")]
        reserve_floor: Nat,
    },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AmountAboveMax {
                max_usd_cents,
                usd_cents,
            } => write!(
                f,
                "amount {usd_cents} USD cents is above the maximum {max_usd_cents} USD cents"
            ),
            Self::AmountBelowMin {
                min_usd_cents,
                usd_cents,
            } => write!(
                f,
                "amount {usd_cents} USD cents is below the minimum {min_usd_cents} USD cents"
            ),
            Self::BuyerNotAllowed => write!(f, "buyer not allowed"),
            Self::CanisterCyclesLow { balance, min } => write!(
                f,
                "gateway cycles balance {balance} is below its minimum {min}"
            ),
            Self::ReserveShort {
                available,
                requested,
            } => write!(
                f,
                "gateway reserve short: {requested} cycles requested, {available} available"
            ),
            Self::TooManyOpenOrders { max, open } => {
                write!(f, "too many open orders: {open} open, {max} allowed")
            }
            Self::UnboundedGiveaway { reserve_floor } => write!(
                f,
                "gateway reserve floor {reserve_floor} is not set up for sales"
            ),
        }
    }
}

/// Why [`CREATE_ORDER_METHOD`] refused.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum CreateOrderError {
    #[serde(rename = "anonymous")]
    Anonymous,
    #[serde(rename = "cancelledDuringCreation")]
    CancelledDuringCreation,
    #[serde(rename = "destinationNotOwned")]
    DestinationNotOwned,
    #[serde(rename = "idGeneration")]
    IdGeneration,
    #[serde(rename = "notAdmitted")]
    NotAdmitted(Reason),
    /// The rate moved against the buyer between quote and creation: the order
    /// would deliver `quoted`, below the `minimum` the buyer pinned.
    #[serde(rename = "quoteChanged")]
    QuoteChanged { minimum: Nat, quoted: Nat },
    #[serde(rename = "rateUnavailable")]
    RateUnavailable,
    #[serde(rename = "reserveUnavailable")]
    ReserveUnavailable,
    #[serde(rename = "sessionUnavailable")]
    SessionUnavailable(String),
    #[serde(rename = "simulationScaleTooSmall")]
    SimulationScaleTooSmall {
        #[serde(rename = "ledgerFee")]
        ledger_fee: Nat,
        #[serde(rename = "scaledCycles")]
        scaled_cycles: Nat,
    },
    #[serde(rename = "tierBelowFees")]
    TierBelowFees(String),
    #[serde(rename = "unknownTier")]
    UnknownTier(String),
}

impl fmt::Display for CreateOrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anonymous => write!(f, "anonymous callers cannot buy"),
            Self::CancelledDuringCreation => write!(f, "cancelled during creation"),
            Self::DestinationNotOwned => write!(f, "destination account not owned by the caller"),
            Self::IdGeneration => write!(f, "the gateway could not generate an order id"),
            Self::NotAdmitted(reason) => write!(f, "not admitted: {reason}"),
            Self::QuoteChanged { minimum, quoted } => write!(
                f,
                "quote changed: {quoted} cycles would be delivered, below the {minimum} asked for"
            ),
            Self::RateUnavailable => write!(f, "no exchange rate available"),
            Self::ReserveUnavailable => write!(f, "the gateway reserve is unavailable"),
            Self::SessionUnavailable(message) => {
                write!(f, "checkout session unavailable: {message}")
            }
            Self::SimulationScaleTooSmall {
                ledger_fee,
                scaled_cycles,
            } => write!(
                f,
                "simulation scale too small: {scaled_cycles} cycles would not cover the {ledger_fee} ledger fee"
            ),
            Self::TierBelowFees(tier) => write!(f, "tier '{tier}' does not cover its fees"),
            Self::UnknownTier(tier) => write!(f, "unknown tier '{tier}'"),
        }
    }
}

/// Why [`CANCEL_ORDER_METHOD`] refused.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum CancelOrderError {
    #[serde(rename = "alreadyExpired")]
    AlreadyExpired,
    #[serde(rename = "credentialsRefused")]
    CredentialsRefused,
    #[serde(rename = "notCancellable")]
    NotCancellable { status: OrderStatus },
    #[serde(rename = "notFound")]
    NotFound,
    #[serde(rename = "sessionNotClosed")]
    SessionNotClosed,
    #[serde(rename = "settledInFlight")]
    SettledInFlight { status: OrderStatus },
    #[serde(rename = "stripeUnavailable")]
    StripeUnavailable,
}

impl fmt::Display for CancelOrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExpired => write!(f, "the order has already expired"),
            Self::CredentialsRefused => {
                write!(f, "the payment processor refused the gateway's credentials")
            }
            Self::NotCancellable { status } => {
                write!(f, "an order with status '{status}' cannot be cancelled")
            }
            Self::NotFound => write!(f, "order not found"),
            Self::SessionNotClosed => write!(f, "the checkout session could not be closed"),
            Self::SettledInFlight { status } => write!(
                f,
                "the order settled while being cancelled (status '{status}')"
            ),
            Self::StripeUnavailable => write!(f, "the payment processor is unavailable"),
        }
    }
}

/// The `ok` payload of [`CREATE_ORDER_METHOD`].
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct CreatedOrder {
    pub order: Order,
}

/// Result of [`CREATE_ORDER_METHOD`].
#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum CreateOrderResult {
    #[serde(rename = "ok")]
    Ok(CreatedOrder),
    #[serde(rename = "err")]
    Err(CreateOrderError),
}

/// Result of [`CANCEL_ORDER_METHOD`].
#[derive(Clone, Debug, CandidType, Deserialize)]
pub enum CancelOrderResult {
    #[serde(rename = "ok")]
    Ok(Box<Order>),
    #[serde(rename = "err")]
    Err(CancelOrderError),
}

/// One page of [`LIST_ORDERS_METHOD`].
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ListOrdersResult {
    /// The cursor for the next page, or `None` on the last one.
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<OrderId>,
    pub orders: Vec<Order>,
}

#[cfg(test)]
mod tests {
    use candid::{Decode, Encode};

    use super::*;

    #[test]
    fn default_gateway_canister_id_parses() {
        assert_eq!(
            resolve_cycles_gateway_canister_id(Err(VarError::NotPresent)).unwrap(),
            Principal::from_text(CYCLES_GATEWAY_CID).unwrap()
        );
        assert_eq!(
            resolve_cycles_gateway_canister_id(Ok("  ".to_string())).unwrap(),
            Principal::from_text(CYCLES_GATEWAY_CID).unwrap()
        );
    }

    #[test]
    fn environment_overrides_default_gateway_canister() {
        let other = "rrkah-fqaaa-aaaaa-aaaaq-cai";
        assert_eq!(
            resolve_cycles_gateway_canister_id(Ok(other.to_string())).unwrap(),
            Principal::from_text(other).unwrap()
        );
    }

    #[test]
    fn invalid_environment_override_is_an_error() {
        let err =
            resolve_cycles_gateway_canister_id(Ok("not-a-principal".to_string())).unwrap_err();
        assert!(err.contains(CYCLES_GATEWAY_CANISTER_ID_ENV), "{err}");
    }

    /// The gateway's `Order` carries more fields than the CLI names; the
    /// trimmed struct must still decode from the full wire record.
    #[test]
    fn trimmed_order_decodes_from_full_wire_record() {
        #[derive(CandidType, Deserialize)]
        enum WireOwner {
            #[serde(rename = "ii")]
            Ii(Principal),
        }

        #[derive(CandidType, Deserialize)]
        enum WireRail {
            #[serde(rename = "card")]
            Card,
        }

        #[derive(CandidType, Deserialize)]
        struct WirePricing {
            #[serde(rename = "feeBps")]
            fee_bps: Nat,
            #[serde(rename = "usdCents")]
            usd_cents: Nat,
        }

        #[derive(CandidType, Deserialize)]
        struct WireOrder {
            id: String,
            status: OrderStatus,
            #[serde(rename = "lockedCycles")]
            locked_cycles: Nat,
            #[serde(rename = "stripeSessionUrl")]
            stripe_session_url: Option<String>,
            #[serde(rename = "stripeSessionId")]
            stripe_session_id: Option<String>,
            #[serde(rename = "expiresAtNs")]
            expires_at_ns: Option<Int>,
            #[serde(rename = "paidUsdCents")]
            paid_usd_cents: Option<Nat>,
            #[serde(rename = "expiredBy")]
            expired_by: Option<ExpiredBy>,
            #[serde(rename = "abandonedReason")]
            abandoned_reason: Option<String>,
            destination: Destination,
            owner: WireOwner,
            rail: WireRail,
            pricing: WirePricing,
            problems: Vec<String>,
            #[serde(rename = "createdAtNs")]
            created_at_ns: Int,
            #[serde(rename = "updatedAtNs")]
            updated_at_ns: Int,
        }

        let owner = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let wire = WireOrder {
            id: "abc".into(),
            status: OrderStatus::Paid,
            locked_cycles: Nat::from(7_238_000_000_000u64),
            stripe_session_url: Some("https://checkout.example/pay".into()),
            stripe_session_id: Some("cs_test".into()),
            expires_at_ns: Some(Int::from(1_700_000_000_000_000_000i64)),
            paid_usd_cents: Some(Nat::from(1000u32)),
            expired_by: None,
            abandoned_reason: None,
            destination: Destination::CyclesLedgerAccount(Account {
                owner,
                subaccount: None,
            }),
            owner: WireOwner::Ii(owner),
            rail: WireRail::Card,
            pricing: WirePricing {
                fee_bps: Nat::from(290u32),
                usd_cents: Nat::from(1000u32),
            },
            problems: vec![],
            created_at_ns: Int::from(1i64),
            updated_at_ns: Int::from(2i64),
        };

        let bytes = Encode!(&wire).unwrap();
        let order = Decode!(&bytes, Order).unwrap();
        assert_eq!(order.id, "abc");
        assert_eq!(order.status, OrderStatus::Paid);
        assert_eq!(order.locked_cycles, Nat::from(7_238_000_000_000u64));
        assert_eq!(
            order.stripe_session_url.as_deref(),
            Some("https://checkout.example/pay")
        );
        assert_eq!(order.paid_usd_cents, Some(Nat::from(1000u32)));
    }

    #[test]
    fn lowercase_tags_round_trip() {
        let bytes = Encode!(&CreateOrderResult::Err(CreateOrderError::NotAdmitted(
            Reason::TooManyOpenOrders {
                max: Nat::from(1u8),
                open: Nat::from(1u8)
            }
        )))
        .unwrap();
        match Decode!(&bytes, CreateOrderResult).unwrap() {
            CreateOrderResult::Err(CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders {
                max,
                open,
            })) => {
                assert_eq!(max, Nat::from(1u8));
                assert_eq!(open, Nat::from(1u8));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// `quote_for_cycles` answers a record with a `rates` field this crate
    /// does not model, and its outcome tags are lowercase.
    #[test]
    fn cycles_quotes_decode_from_full_wire_record() {
        #[derive(CandidType)]
        struct WireRates {
            fetched_at_ns: Int,
        }
        #[derive(CandidType)]
        struct WireCyclesQuotes {
            quotes: Vec<CyclesQuote>,
            rates: Option<WireRates>,
        }
        let bytes = Encode!(&WireCyclesQuotes {
            quotes: vec![
                CyclesQuote {
                    cycles: Nat::from(5u8),
                    outcome: CyclesQuoteOutcome::Ok {
                        cycles_quoted: Nat::from(6u8),
                        fee_cents: Nat::from(51u8),
                        net_cents: Nat::from(651u16),
                        usd_cents: Nat::from(702u16),
                    },
                },
                CyclesQuote {
                    cycles: Nat::from(1u8),
                    outcome: CyclesQuoteOutcome::Unpriceable(Unpriceable::StripeFee),
                },
                CyclesQuote {
                    cycles: Nat::from(2u8),
                    outcome: CyclesQuoteOutcome::Stale,
                },
            ],
            rates: Some(WireRates {
                fetched_at_ns: Int::from(7),
            }),
        })
        .unwrap();
        let decoded = Decode!(&bytes, CyclesQuotes).unwrap();
        assert_eq!(decoded.quotes.len(), 3);
        let CyclesQuoteOutcome::Ok { usd_cents, .. } = &decoded.quotes[0].outcome else {
            panic!("unexpected {:?}", decoded.quotes[0].outcome);
        };
        assert_eq!(*usd_cents, Nat::from(702u16));
        assert_eq!(
            decoded.quotes[1].outcome,
            CyclesQuoteOutcome::Unpriceable(Unpriceable::StripeFee)
        );
        assert_eq!(decoded.quotes[2].outcome, CyclesQuoteOutcome::Stale);
    }
}
