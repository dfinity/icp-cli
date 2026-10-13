//! The CyclePay backend's Candid interface, as far as `icp cycles buy` uses it.
//!
//! [CyclePay](https://www.cyclepay.org) sells cycles for a card payment. Its
//! backend canister prices and owns every order. The card itself is charged on
//! a Stripe Payment Link that carries the order's reference, and a separate
//! Stripe gateway canister reports the payment back to the backend, which
//! then mints the cycles. The CLI talks to the backend only: it creates the
//! order as the buyer, hands out the Payment Link, and reads the order back
//! until it settles.
//!
//! The backend is a Rust canister, so names are as written here on the wire.
//! Candid lets a record carry more fields than its reader names, so each
//! record below declares only what the CLI reads.

use std::env::{self, VarError};
use std::fmt;

use candid::{CandidType, Principal};
use serde::Deserialize;
use url::Url;

/// Default cycles gateway: the CyclePay backend canister on mainnet.
///
/// Overridable at runtime via the [`CYCLES_GATEWAY_CANISTER_ID_ENV`]
/// environment variable — see [`cycles_gateway_canister_id`].
pub const CYCLES_GATEWAY_CID: &str = "2bwus-iiaaa-aaaac-bee3a-cai";

/// Environment variable that overrides [`CYCLES_GATEWAY_CID`].
pub const CYCLES_GATEWAY_CANISTER_ID_ENV: &str = "ICP_CYCLES_GATEWAY_CANISTER_ID";

/// The Stripe Payment Link for signed-in orders, as CyclePay's agent index
/// (`https://www.cyclepay.org/llms.txt`) publishes it. Only a session created
/// from CyclePay's own links counts: a payment made anywhere else is refunded,
/// never delivered.
///
/// The backend does not report its link, so the CLI carries it. Overridable
/// via [`PAYMENT_LINK_URL_ENV`] — see [`payment_link_url`].
pub const PAYMENT_LINK_URL: &str = "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00";

/// Environment variable that overrides [`PAYMENT_LINK_URL`].
pub const PAYMENT_LINK_URL_ENV: &str = "ICP_CYCLES_PAYMENT_LINK_URL";

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

/// The least a signed-in order may be, in US cents: the backend's
/// `min_order_usd_cents`. A buyer cannot read the live value (the query that
/// reports it is for operators), so the published default is carried here.
pub const MIN_ORDER_USD_CENTS: u64 = 100;

/// The most a signed-in order may be, in US cents: the backend's
/// `max_order_usd_cents`, likewise.
pub const MAX_ORDER_USD_CENTS: u64 = 20_000;

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

/// The host every Stripe Payment Link is served from, test-mode ones
/// included.
pub const PAYMENT_LINK_HOST: &str = "buy.stripe.com";

/// Resolve the Payment Link orders are paid on, for the gateway at hand.
///
/// [`PAYMENT_LINK_URL_ENV`] when set and non-empty; otherwise
/// [`PAYMENT_LINK_URL`] for CyclePay's own gateway (`cyclepay`), and an
/// error for any other, since a payment on CyclePay's link goes to CyclePay,
/// which would refund it as an order it does not have.
///
/// A card is typed on this page, so an override must be a Payment Link and
/// nothing else: `https://buy.stripe.com/<link>`, with no credentials, port,
/// query or fragment, so that what is checked here is what the browser opens
/// once the order's reference and amount are added. A configured-but-invalid
/// value must never fall back to the built-in link.
pub fn payment_link_url(cyclepay: bool) -> Result<Url, String> {
    resolve_payment_link_url(env::var(PAYMENT_LINK_URL_ENV), cyclepay)
}

fn resolve_payment_link_url(env: Result<String, VarError>, cyclepay: bool) -> Result<Url, String> {
    match env {
        Ok(value) if !value.trim().is_empty() => {
            let invalid = |why: &str| format!("invalid {PAYMENT_LINK_URL_ENV}: {why}");
            let url = Url::parse(value.trim()).map_err(|e| invalid(&e.to_string()))?;
            if url.scheme() != "https" {
                return Err(invalid("not an https URL"));
            }
            let is_payment_link = url.host_str() == Some(PAYMENT_LINK_HOST)
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().len() > 1;
            if !is_payment_link {
                return Err(invalid(&format!(
                    "not a Stripe Payment Link (https://{PAYMENT_LINK_HOST}/<link>)"
                )));
            }
            Ok(url)
        }
        Err(VarError::NotUnicode(_)) => {
            Err(format!("invalid {PAYMENT_LINK_URL_ENV}: not valid Unicode"))
        }
        Ok(_) | Err(VarError::NotPresent) if cyclepay => {
            Ok(Url::parse(PAYMENT_LINK_URL).expect("PAYMENT_LINK_URL is a valid URL"))
        }
        Ok(_) | Err(VarError::NotPresent) => Err(format!(
            "the gateway is not CyclePay's, so its orders are not paid on CyclePay's Payment \
             Link; set {PAYMENT_LINK_URL_ENV} to the one they are paid on"
        )),
    }
}

/// The page one order is paid on: the Payment Link carrying the order's
/// reference, with the amount prefilled in cents. Stripe's page lets the
/// buyer change the amount; the order is delivered for what is paid.
pub fn checkout_url(payment_link: &Url, client_reference_id: &str, amount_usd_cents: u64) -> Url {
    let mut url = payment_link.clone();
    url.query_pairs_mut()
        .append_pair("client_reference_id", client_reference_id)
        .append_pair("prefilled_amount", &amount_usd_cents.to_string());
    url
}

/// The reference the backend attaches to an order's Payment Link session:
/// `o_<order id>`, as `CreatedOrder::client_reference_id` reports it. Rebuilt
/// here for an order read back later, whose view does not carry it.
pub fn order_reference(order_id: &str) -> String {
    format!("o_{order_id}")
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
/// reads; `destination` is left out.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct CreatedOrder {
    pub order_id: String,
    /// What the Payment Link URL must carry: `o_<order id>`.
    pub client_reference_id: String,
    /// What the order asked for, in US cents.
    pub amount_usd_cents: u64,
    /// What the payment funds after the card fee, in US cents.
    pub net_usd_cents: u64,
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

    #[test]
    fn payment_link_defaults_and_overrides() {
        assert_eq!(
            resolve_payment_link_url(Err(VarError::NotPresent), true)
                .unwrap()
                .as_str(),
            PAYMENT_LINK_URL
        );
        for cyclepay in [true, false] {
            assert_eq!(
                resolve_payment_link_url(
                    Ok(" https://buy.stripe.com/test_abc ".to_string()),
                    cyclepay
                )
                .unwrap()
                .as_str(),
                "https://buy.stripe.com/test_abc"
            );
        }
        // Another gateway has no default: CyclePay's link pays CyclePay.
        let err = resolve_payment_link_url(Err(VarError::NotPresent), false).unwrap_err();
        assert!(err.contains(PAYMENT_LINK_URL_ENV), "{err}");
        let err = resolve_payment_link_url(Ok("http://buy.stripe.com/abc".to_string()), true)
            .unwrap_err();
        assert!(err.contains("https"), "{err}");
        let err = resolve_payment_link_url(Ok("not a url".to_string()), true).unwrap_err();
        assert!(err.contains(PAYMENT_LINK_URL_ENV), "{err}");
    }

    /// Only a Payment Link is a page to send a card to, checked as the
    /// browser will read it: the host after any credentials, and no query of
    /// its own to sit beside, or ahead of, the order's reference.
    #[test]
    fn a_payment_link_override_is_a_payment_link() {
        for bad in [
            "https://evil.example/pay",
            "https://buy.stripe.com@evil.example/abc",
            "https://user:pass@buy.stripe.com/abc",
            "https://buy.stripe.com.evil.example/abc",
            "https://buy.stripe.com:8443/abc",
            "https://buy.stripe.com/abc?client_reference_id=o_someone-else",
            "https://buy.stripe.com/abc#x",
            "https://buy.stripe.com/",
        ] {
            let err = resolve_payment_link_url(Ok(bad.to_string()), true).unwrap_err();
            assert!(err.contains("not a Stripe Payment Link"), "{bad}: {err}");
        }
    }

    #[test]
    fn checkout_url_carries_reference_and_amount() {
        let link = Url::parse(PAYMENT_LINK_URL).unwrap();
        assert_eq!(
            checkout_url(&link, &order_reference("co-12"), 2500).as_str(),
            format!("{PAYMENT_LINK_URL}?client_reference_id=o_co-12&prefilled_amount=2500")
        );
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
            order_id: String,
            amount_usd_cents: u64,
            expires_at: u64,
        }

        let owner = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let bytes = Encode!(&WireCreatedOrder {
            destination: WireDestination::Ledger(owner),
            client_reference_id: "o_co-1".into(),
            net_usd_cents: 2398,
            order_id: "co-1".into(),
            amount_usd_cents: 2500,
            expires_at: 1_800_000_000,
        })
        .unwrap();
        let created = Decode!(&bytes, CreatedOrder).unwrap();
        assert_eq!(created.order_id, "co-1");
        assert_eq!(created.client_reference_id, "o_co-1");
        assert_eq!(created.net_usd_cents, 2398);

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
