//! The CyclePay backend's Candid interface, as far as `icp cycles buy` uses it.
//!
//! [CyclePay](https://www.cyclepay.org) sells cycles for a card payment. Its
//! backend canister prices and owns every order. The card itself is charged on
//! a Stripe Payment Link that carries the order's reference, and a separate
//! Stripe gateway canister reports the payment back to the backend, which
//! then mints the cycles. The CLI talks to the backend only: it reads the
//! terms the backend publishes, creates the order as the buyer, hands out the
//! page the backend says to pay on, and reads the order back until it
//! settles.
//!
//! The interface is what makes a canister a cycles gateway: any canister
//! serving these four methods can stand in for CyclePay's backend, and the
//! CLI carries nothing of its own but the default gateway's canister id. The
//! page to pay on, the order limits and the fees all come from the gateway.
//!
//! The backend is a Rust canister, so names are as written here on the wire.
//! Candid lets a record carry more fields than its reader names, so each
//! record below declares only what the CLI reads.

use std::env::{self, VarError};
use std::fmt;

use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

/// Default cycles gateway: the CyclePay backend canister on mainnet.
///
/// Overridable at runtime via the [`CYCLES_GATEWAY_CANISTER_ID_ENV`]
/// environment variable — see [`cycles_gateway_canister_id`].
pub const CYCLES_GATEWAY_CID: &str = "2bwus-iiaaa-aaaac-bee3a-cai";

/// Environment variable that overrides [`CYCLES_GATEWAY_CID`].
pub const CYCLES_GATEWAY_CANISTER_ID_ENV: &str = "ICP_CYCLES_GATEWAY_CANISTER_ID";

/// Whether `gateway` is CyclePay's own backend, [`CYCLES_GATEWAY_CID`]: the
/// one gateway the CLI knows anything about beyond its interface, which is
/// on mainnet and nowhere else.
pub fn is_cyclepay(gateway: Principal) -> bool {
    gateway
        == Principal::from_text(CYCLES_GATEWAY_CID)
            .expect("CYCLES_GATEWAY_CID is a valid principal")
}

/// Public query: `() -> (CardTerms)`. What a buyer needs to pay by card: the
/// Payment Link, the order limits and the fees.
pub const CARD_TERMS_METHOD: &str = "get_card_terms";

/// Public query: `() -> (XdrRateStatus)`. The XDR/USD rate the backend prices
/// every order with, and how fresh it is.
pub const XDR_RATE_STATUS_METHOD: &str = "get_xdr_rate_status";

/// Update creating an order for the caller:
/// `(CreateOrderArgs) -> (variant { Ok : CreatedOrder; Err : OrderError })`.
pub const CREATE_ORDER_METHOD: &str = "create_order";

/// Owner-scoped query: `(text) -> (opt OrderStatusView)`. `null` for an order
/// the caller does not own, one that never existed, and one forgotten after
/// expiring unpaid.
pub const ORDER_STATUS_METHOD: &str = "order_status";

/// Resolve the cycles gateway canister id to talk to.
///
/// Uses the value of the [`CYCLES_GATEWAY_CANISTER_ID_ENV`] variable when set
/// and non-empty, otherwise falls back to [`CYCLES_GATEWAY_CID`]. Returns an
/// error when the override is set but invalid — either not a valid principal,
/// or not valid Unicode. Only an absent or empty (whitespace-only) variable
/// uses the default: a configured-but-invalid value must never silently route
/// an order to the built-in gateway.
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

/// The reply of [`CARD_TERMS_METHOD`]: what a buyer needs to pay by card, as
/// the gateway publishes it. Every figure the CLI quotes with comes from
/// here, so a gateway can change any of them without a CLI release. Only the
/// fields the CLI reads; the Payment Link itself is left out, since every
/// order comes with its own page.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct CardTerms {
    /// `create_order` refuses new orders (`CARDS_UNAVAILABLE`) while true:
    /// deliveries are paused, or card payments are off.
    pub deliveries_paused: bool,
    /// The least a signed-in order may be, in US cents.
    pub min_order_usd_cents: u64,
    /// The most a signed-in order may be, in US cents.
    pub max_order_usd_cents: u64,
    /// The card fee taken off the amount paid:
    /// `fee = amount * fee_bps / 10_000 + fee_fixed_usd_cents`, rounded to
    /// the cent as the backend rounds, so a client can quote `net_usd_cents`
    /// before it orders.
    pub fee_bps: u64,
    /// The fee's fixed part, in US cents.
    pub fee_fixed_usd_cents: u64,
    /// Taken off a delivery to a Cycles Ledger account, in cycles. A
    /// canister top-up pays none.
    pub ledger_delivery_fee_cycles: Nat,
}

/// Where the cycles go.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum OrderDestination {
    /// Top up this canister.
    Canister(Principal),
    /// Credit a Cycles Ledger account: the buyer's own, unless a beneficiary
    /// is named.
    Ledger,
}

/// Argument of [`CREATE_ORDER_METHOD`].
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct CreateOrderArgs {
    pub destination: OrderDestination,
    /// Ledger only: whose account is credited. `None` is the caller's.
    pub beneficiary: Option<Principal>,
    /// A console project the order belongs to. `None` for an order of the
    /// caller's own.
    pub project_id: Option<String>,
    /// What the buyer will pay, in US cents.
    pub amount_usd_cents: u64,
    /// Required (`Some(true)`) to top up a system canister. Irrelevant to a
    /// ledger destination.
    pub confirm_system_canister: Option<bool>,
}

/// The `Ok` payload of [`CREATE_ORDER_METHOD`]. Only the fields the CLI
/// reads; `destination` and the Payment Link reference are left out.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct CreatedOrder {
    pub order_id: String,
    /// What the order asked for, in US cents.
    pub amount_usd_cents: u64,
    /// What the payment funds after the card fee, in US cents.
    pub net_usd_cents: u64,
    /// Where to pay: the gateway's Payment Link carrying this order's
    /// reference and amount, to be opened as it is.
    pub payment_url: String,
    /// When the order is forgotten if unpaid, in seconds since the Unix
    /// epoch.
    pub expires_at: u64,
}

/// Why [`CREATE_ORDER_METHOD`] refused. Nothing is written for any of them.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct OrderError {
    /// A fixed code, matched on rather than `message`: see
    /// [`OrderErrorKind`].
    pub code: String,
    pub message: String,
}

impl OrderError {
    pub fn kind(&self) -> OrderErrorKind {
        OrderErrorKind::parse(&self.code)
    }
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// [`OrderError::code`], as the backend documents its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderErrorKind {
    Anonymous,
    /// The backend is paused: nothing is sold.
    Paused,
    /// Card payments are off.
    CardsUnavailable,
    AmountOutOfRange,
    CanisterIdInvalid,
    DestinationBlocked,
    SystemCanisterWarning,
    BeneficiaryNotAllowed,
    RateLimited,
    TooManyOpenOrders,
    NotProjectMember,
    ProjectsUnavailable,
    /// A code this CLI does not know.
    Unknown,
}

impl OrderErrorKind {
    fn parse(code: &str) -> Self {
        match code {
            "ANONYMOUS" => Self::Anonymous,
            "PAUSED" => Self::Paused,
            "CARDS_UNAVAILABLE" => Self::CardsUnavailable,
            "AMOUNT_OUT_OF_RANGE" => Self::AmountOutOfRange,
            "CANISTER_ID_INVALID" => Self::CanisterIdInvalid,
            "DESTINATION_BLOCKED" => Self::DestinationBlocked,
            "SYSTEM_CANISTER_WARNING" => Self::SystemCanisterWarning,
            "BENEFICIARY_NOT_ALLOWED" => Self::BeneficiaryNotAllowed,
            "RATE_LIMITED" => Self::RateLimited,
            "TOO_MANY_OPEN_ORDERS" => Self::TooManyOpenOrders,
            "NOT_PROJECT_MEMBER" => Self::NotProjectMember,
            "PROJECTS_UNAVAILABLE" => Self::ProjectsUnavailable,
            _ => Self::Unknown,
        }
    }
}

/// Result of [`CREATE_ORDER_METHOD`].
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum CreateOrderResult {
    Ok(CreatedOrder),
    Err(OrderError),
}

/// A card order's state, as the backend reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum CardOrderStatus {
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
}

/// Where an order's cycles go, as the backend records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum Destination {
    /// Top up this canister.
    Canister(Principal),
    /// Credit this principal's Cycles Ledger account.
    Ledger(Principal),
}

/// An order as its owner sees it: the reply of [`ORDER_STATUS_METHOD`]. Only
/// the fields the CLI reads; the owner and Stripe identifiers are left out.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct OrderStatusView {
    pub order_id: String,
    /// Optional on this side only: a state added to the backend after this
    /// CLI was built reads as `None` (Candid's opt rule) rather than failing
    /// every reply, so CyclePay can add one without breaking shipped CLIs.
    pub status: Option<CardOrderStatus>,
    /// What the order asked for, in US cents.
    pub amount_usd_cents: u64,
    /// What the payment funds after the card fee, in US cents: the order's
    /// amount until paid, then the amount paid.
    pub net_usd_cents: u64,
    /// What was paid, once paid, in US cents.
    pub paid_usd_cents: Option<u64>,
    /// Exact cycles, once delivered.
    pub cycles_delivered: Option<u64>,
    /// Refunded so far, in US cents.
    pub amount_refunded_usd_cents: u64,
    /// Why delivery waits or stopped, or why a refund is owed.
    pub detail: Option<String>,
    /// Until paid: when the order is forgotten, in seconds since the Unix
    /// epoch.
    pub expires_at: Option<u64>,
    /// Where the cycles go. Optional on this side only, as `status` is: an
    /// order made on CyclePay's site can be a canister top-up, or for
    /// another principal, and a kind added later reads as `None`.
    pub destination: Option<Destination>,
}

/// The reply of [`XDR_RATE_STATUS_METHOD`]. Only the fields the CLI reads;
/// the rate's provenance and the refresh diagnostics are left out.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct XdrRateStatus {
    /// US dollars per XDR, times 10,000. One XDR is one trillion cycles.
    pub xdr_usd_x10000: u64,
    /// No refresh for longer than the backend tolerates. The rate is still
    /// the one every order is priced with.
    pub stale: bool,
    /// Seconds since the rate was set, when known.
    pub age_seconds: Option<u64>,
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

    /// The backend's replies carry more fields than the CLI names; the
    /// trimmed records must still decode from the full wire records.
    #[test]
    fn trimmed_records_decode_from_full_wire_records() {
        #[derive(CandidType)]
        enum WireDestination {
            Ledger(Principal),
        }

        #[derive(CandidType)]
        struct WireCreatedOrder {
            destination: WireDestination,
            client_reference_id: String,
            net_usd_cents: u64,
            payment_url: String,
            order_id: String,
            amount_usd_cents: u64,
            expires_at: u64,
        }

        let owner = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let bytes = Encode!(&WireCreatedOrder {
            destination: WireDestination::Ledger(owner),
            client_reference_id: "o_co-1".into(),
            net_usd_cents: 2398,
            payment_url:
                "https://buy.stripe.com/test_link?client_reference_id=o_co-1&prefilled_amount=2500"
                    .into(),
            order_id: "co-1".into(),
            amount_usd_cents: 2500,
            expires_at: 1_800_000_000,
        })
        .unwrap();
        let created = Decode!(&bytes, CreatedOrder).unwrap();
        assert_eq!(created.order_id, "co-1");
        assert_eq!(created.net_usd_cents, 2398);
        assert!(created.payment_url.ends_with("prefilled_amount=2500"));

        // The terms as the backend's `.did` orders them, which is not the
        // order the CLI declares them in.
        #[derive(CandidType)]
        struct WireCardTerms {
            deliveries_paused: bool,
            payment_link: String,
            max_order_usd_cents: u64,
            fee_bps: u64,
            min_order_usd_cents: u64,
            ledger_delivery_fee_cycles: Nat,
            fee_fixed_usd_cents: u64,
        }

        let bytes = Encode!(&WireCardTerms {
            deliveries_paused: false,
            payment_link: "https://buy.stripe.com/test_link".into(),
            max_order_usd_cents: 20_000,
            fee_bps: 290,
            min_order_usd_cents: 100,
            ledger_delivery_fee_cycles: Nat::from(200_000_000u64),
            fee_fixed_usd_cents: 30,
        })
        .unwrap();
        let terms = Decode!(&bytes, CardTerms).unwrap();
        assert_eq!(
            terms,
            CardTerms {
                deliveries_paused: false,
                min_order_usd_cents: 100,
                max_order_usd_cents: 20_000,
                fee_bps: 290,
                fee_fixed_usd_cents: 30,
                ledger_delivery_fee_cycles: Nat::from(200_000_000u64),
            }
        );

        #[derive(CandidType)]
        struct WireOrderStatusView {
            payment_intent: Option<String>,
            status: CardOrderStatus,
            cycles_delivered: Option<u64>,
            destination: WireDestination,
            session_id: Option<String>,
            owner: Principal,
            net_usd_cents: u64,
            created_at: u64,
            detail: Option<String>,
            amount_refunded_usd_cents: u64,
            paid_at: Option<u64>,
            order_id: String,
            project_id: Option<String>,
            amount_usd_cents: u64,
            expires_at: Option<u64>,
            paid_usd_cents: Option<u64>,
        }

        let bytes = Encode!(&Some(WireOrderStatusView {
            payment_intent: Some("pi_1".into()),
            status: CardOrderStatus::Delivered,
            cycles_delivered: Some(17_730_000_000_000),
            destination: WireDestination::Ledger(owner),
            session_id: Some("cs_live_1".into()),
            owner,
            net_usd_cents: 2398,
            created_at: 1_700_000_000,
            detail: None,
            amount_refunded_usd_cents: 0,
            paid_at: Some(1_700_000_100),
            order_id: "co-1".into(),
            project_id: None,
            amount_usd_cents: 2500,
            expires_at: None,
            paid_usd_cents: Some(2500),
        }))
        .unwrap();
        let view = Decode!(&bytes, Option<OrderStatusView>).unwrap().unwrap();
        assert_eq!(view.destination, Some(Destination::Ledger(owner)));
        assert_eq!(view.status, Some(CardOrderStatus::Delivered));
        assert_eq!(view.cycles_delivered, Some(17_730_000_000_000));
        assert_eq!(view.paid_usd_cents, Some(2500));

        // A state the backend adds later reads as no status, not as a reply
        // that cannot be decoded.
        #[derive(CandidType)]
        enum FutureStatus {
            Disputed,
        }
        #[derive(CandidType)]
        struct FutureView {
            status: FutureStatus,
            order_id: String,
            amount_usd_cents: u64,
            net_usd_cents: u64,
            paid_usd_cents: Option<u64>,
            cycles_delivered: Option<u64>,
            amount_refunded_usd_cents: u64,
            detail: Option<String>,
            expires_at: Option<u64>,
        }
        let bytes = Encode!(&Some(FutureView {
            status: FutureStatus::Disputed,
            order_id: "co-1".into(),
            amount_usd_cents: 2500,
            net_usd_cents: 2398,
            paid_usd_cents: Some(2500),
            cycles_delivered: None,
            amount_refunded_usd_cents: 0,
            detail: None,
            expires_at: None,
        }))
        .unwrap();
        let view = Decode!(&bytes, Option<OrderStatusView>).unwrap().unwrap();
        assert_eq!(view.status, None);
        assert_eq!(view.order_id, "co-1");

        #[derive(CandidType)]
        enum WireSource {
            Xrc,
        }

        #[derive(CandidType)]
        struct WireXdrRateStatus {
            last_error: Option<String>,
            updated_at: Option<u64>,
            source: WireSource,
            xdr_usd_x10000: u64,
            stale: bool,
            auto_refresh: bool,
            consecutive_failures: u32,
            refresh_interval_seconds: u64,
            age_seconds: Option<u64>,
        }

        let bytes = Encode!(&WireXdrRateStatus {
            last_error: None,
            updated_at: Some(1_700_000_000),
            source: WireSource::Xrc,
            xdr_usd_x10000: 13_520,
            stale: false,
            auto_refresh: true,
            consecutive_failures: 0,
            refresh_interval_seconds: 21_600,
            age_seconds: Some(120),
        })
        .unwrap();
        let rate = Decode!(&bytes, XdrRateStatus).unwrap();
        assert_eq!(rate.xdr_usd_x10000, 13_520);
        assert!(!rate.stale);
        assert_eq!(rate.age_seconds, Some(120));
    }

    #[test]
    fn order_error_codes_are_recognized() {
        let error = OrderError {
            code: "RATE_LIMITED".into(),
            message: "5 a minute".into(),
        };
        assert_eq!(error.kind(), OrderErrorKind::RateLimited);
        assert_eq!(error.to_string(), "RATE_LIMITED: 5 a minute");
        assert_eq!(
            OrderError {
                code: "SOMETHING_NEW".into(),
                message: String::new(),
            }
            .kind(),
            OrderErrorKind::Unknown
        );
    }
}
