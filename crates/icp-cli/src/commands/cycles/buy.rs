use std::io::{IsTerminal, stderr, stdin, stdout};

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::Args;
use dialoguer::Confirm;
use icp_app::context::{Context, NetworkSelection};
use icp_app::identity::IdentitySelection;
use icp_app::identity::manifest::IdentityDefaults;
use icp_app::operations::token::balance::get_raw_balance;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icp_project::calls::CanisterCalls;
use icp_project::host::EnvironmentSelection;
use icp_project::network::RootKeySpec;
use icp_project::operations::task::Reporter;
use icp_project::parsers::CyclesAmount;
use icp_project::signal::stop_signal;
use num_traits::ToPrimitive;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};

use crate::commands::args::{TokenCommandArgs, TokenCommandSelections};
use crate::cyclepay::format_cycles;
use crate::cyclepay::interface::{
    CYCLES_GATEWAY_CID, OrderError, OrderErrorKind, cycles_gateway_canister_id, payment_link_url,
};
use crate::cyclepay::money::{Currency, Money};
use crate::cyclepay::purchase::{
    self as purchase, CyclesPurchaseError, Gateway, OrderStatus, OrderView, Quote, Target,
    WaitOptions, now_secs,
};
use crate::render::rendered;

/// Buy cycles with a card.
///
/// Creates an order on the CyclePay backend canister as the current identity,
/// prints the Payment Link to pay on (and opens it in a browser when run from
/// a terminal), then waits for the cycles to land on the identity's own
/// cycles-ledger account. Fund a canister from there with `icp canister
/// top-up`. Say either how much to spend (--amount, in the currency given by
/// --currency) or how many cycles to receive (--cycles); the latter pays the
/// least amount that buys them once the card fee is taken out. CyclePay
/// accepts USD only, from 1 to 200 per order.
///
/// Exactly one of --amount, --cycles or --resume must be given.
#[derive(Debug, Args)]
pub(crate) struct BuyArgs {
    /// Amount to spend, as a decimal (e.g. 10 or 12.50), in the currency given by --currency
    #[arg(
        long,
        value_name = "AMOUNT",
        required_unless_present_any = ["cycles", "resume"],
        conflicts_with_all = ["cycles", "resume"]
    )]
    pub(crate) amount: Option<String>,

    /// Cycles to receive; the amount to pay is worked out from the current rate, card fee included.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, value_name = "CYCLES", conflicts_with = "resume")]
    pub(crate) cycles: Option<CyclesAmount>,

    /// ISO 4217 code of the currency to pay in. CyclePay accepts USD only.
    #[arg(long, value_name = "CODE", default_value = "USD")]
    pub(crate) currency: Currency,

    /// Continue waiting on an existing order instead of creating a new one
    #[arg(long, value_name = "ORDER_ID")]
    pub(crate) resume: Option<String>,

    /// Canister id of the cycles gateway to buy through: the CyclePay backend.
    /// Defaults to the mainnet backend, or to ICP_CYCLES_GATEWAY_CANISTER_ID when set.
    #[arg(long, value_name = "CANISTER_ID")]
    pub(crate) gateway: Option<Principal>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// Skip confirmation prompts
    #[arg(long, short)]
    pub(crate) yes: bool,

    /// Do not open the Payment Link in a browser
    #[arg(long)]
    pub(crate) no_open: bool,

    /// Print the Payment Link and return without waiting for the order to complete
    #[arg(long)]
    pub(crate) no_wait: bool,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the Payment Link, then the final status
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Human,
    Quiet,
    Json,
}

impl Output {
    fn of(args: &BuyArgs) -> Self {
        if args.json {
            Self::Json
        } else if args.quiet {
            Self::Quiet
        } else {
            Self::Human
        }
    }
}

pub(crate) async fn exec(ctx: &Context, args: &BuyArgs) -> Result<(), anyhow::Error> {
    // A malformed amount should fail before any identity is unlocked or any
    // network reached.
    let target = match (args.amount.as_deref(), args.cycles.as_ref()) {
        (Some(amount), _) => Some(Target::Spend(Money::parse_decimal(amount, args.currency)?)),
        (None, Some(cycles)) => {
            if cycles.get() == 0 {
                bail!("--cycles must be greater than zero");
            }
            Some(Target::Receive {
                cycles: cycles.get(),
                currency: args.currency,
            })
        }
        (None, None) => None,
    };

    let selections = args.token_command_args.selections();
    if matches!(selections.identity, IdentitySelection::Anonymous) {
        bail!("cycles cannot be bought as the anonymous identity; choose one with --identity");
    }

    let canister = match args.gateway {
        Some(gateway) => gateway,
        None => cycles_gateway_canister_id().map_err(|message| anyhow!(message))?,
    };
    let gateway = Gateway {
        canister,
        payment_link: payment_link_url(canister.to_text() == CYCLES_GATEWAY_CID)
            .map_err(|message| anyhow!(message))?,
    };

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let buyer = agent
        .get_principal()
        .map_err(|message| anyhow!("failed to get the identity's principal: {message}"))?;
    if buyer == Principal::anonymous() {
        bail!("cycles cannot be bought as the anonymous identity; choose one with --identity");
    }
    let calls = icp_app::calls::calls(agent.clone(), None)?;
    let calls = calls.as_ref();
    let output = Output::of(args);

    // Whether the Payment Link was handed out: an order that is payable right
    // now. Anything else has nothing to pay, and `--no-wait` must still say
    // where it stands.
    let (order, payable) = match &args.resume {
        Some(id) => {
            let order = purchase::get_order(calls, &gateway, id).await?;
            let payable = order.status == OrderStatus::AwaitingPayment && !past_deadline(&order);
            if payable {
                announce_payable(&order, None, output, args.no_open);
            } else if output == Output::Human {
                match order.status {
                    OrderStatus::AwaitingPayment => info!(
                        "Order {} passed its payment deadline at {}; a payment now would be refunded.",
                        order.id,
                        order.expires_at.map(format_secs).unwrap_or_default()
                    ),
                    status => info!("Order {} is {status}", order.id),
                }
            }
            (order, payable)
        }
        None => {
            let target =
                target.expect("clap requires --amount or --cycles unless --resume is given");
            let identity = identity_name(ctx, &selections.identity).await;
            let (order, quote) =
                create_order(calls, &gateway, buyer, identity.as_deref(), target, args).await?;
            announce_payable(&order, Some(&quote), output, args.no_open);
            (order, true)
        }
    };

    // Only the identity that made an order can read it, on the network its
    // gateway is on: the command that resumes it says both.
    let resume = |id: &str| {
        format!(
            "icp cycles buy --resume {}{}",
            shell_word(id),
            selection_flags(&selections, args.gateway)
        )
    };

    if args.no_wait {
        if !payable {
            print_final(&order, None, output)?;
        }
        if output == Output::Human && !order.status.is_terminal() {
            info!(
                "Not waiting. Run `{}` to wait for delivery.",
                resume(&order.id)
            );
        }
        return Ok(());
    }

    let resume = resume(&order.id);
    wait_and_report(
        calls, &agent, &gateway, buyer, order, output, ctx.debug, &resume,
    )
    .await
}

/// The selections a run was given, as flags for the command that resumes
/// its order.
fn selection_flags(selections: &TokenCommandSelections, gateway: Option<Principal>) -> String {
    let mut flags = String::new();
    match &selections.network {
        NetworkSelection::Named(name) => flags.push_str(&format!(" -n {}", shell_word(name))),
        NetworkSelection::Url(url, root_key) => {
            flags.push_str(&format!(" -n {}", shell_word(url.as_str())));
            let root_key = match root_key {
                RootKeySpec::Mainnet => "mainnet".to_owned(),
                RootKeySpec::Fetch => "fetch".to_owned(),
                RootKeySpec::Explicit(key) => hex::encode(key),
            };
            flags.push_str(&format!(" -k {root_key}"));
        }
        NetworkSelection::Default => {}
    }
    if let EnvironmentSelection::Named(name) = &selections.environment {
        flags.push_str(&format!(" -e {}", shell_word(name)));
    }
    if let IdentitySelection::Named(name) = &selections.identity {
        flags.push_str(&format!(" --identity {}", shell_word(name)));
    }
    if let Some(gateway) = gateway {
        flags.push_str(&format!(" --gateway {gateway}"));
    }
    flags
}

/// `value` as one shell word: as it is when nothing in it needs quoting.
fn shell_word(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@=+".contains(c));
    if plain {
        value.to_owned()
    } else {
        icp_project::operations::create::shell_quote(value)
    }
}

/// Whether an order's payment deadline has passed on this machine's clock.
fn past_deadline(order: &OrderView) -> bool {
    order
        .expires_at
        .is_some_and(|deadline| deadline < now_secs())
}

/// Quote, confirm, create. Returns the order with the quote it was placed on.
async fn create_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
    target: Target,
    args: &BuyArgs,
) -> Result<(OrderView, Quote), anyhow::Error> {
    let quote = purchase::quote_target(calls, gateway, target).await?;
    // A script that said --yes reads the figures off --json; -q asks for
    // nothing but the link.
    let speak = !args.yes || !(args.quiet || args.json);
    confirm(&quote, target, gateway, buyer, identity, args.yes, speak)?;
    match purchase::place_order(calls, gateway, quote.amount).await {
        Ok(order) => Ok((order, quote)),
        Err(CyclesPurchaseError::OrderRefused { error }) => {
            bail!("{}", refusal_message(&error))
        }
        Err(error) => Err(error.into()),
    }
}

/// Show what is about to happen and ask. Without a terminal there is nobody
/// to ask, so `--yes` is required.
#[allow(clippy::too_many_arguments)]
fn confirm(
    quote: &Quote,
    target: Target,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
    yes: bool,
    speak: bool,
) -> Result<(), anyhow::Error> {
    if !speak {
        return Ok(());
    }
    let identity = identity
        .map(|name| format!(" (identity '{name}')"))
        .unwrap_or_default();
    info!("Buyer:    {buyer}{identity}");
    info!("Gateway:  {} (CyclePay backend canister)", gateway.canister);
    if let Target::Receive { cycles, .. } = target {
        info!("Wanted:   at least {}", format_cycles(cycles));
    }
    info!("Quote:    {}", quote_line(quote));
    if quote.rate.stale {
        let age = quote
            .rate
            .age
            .map(|age| format!(" for {}", format_age(age)))
            .unwrap_or_default();
        warn!(
            "CyclePay has not refreshed its exchange rate{age}; the quote may be off, and the \
             cycles delivered follow the rate when the payment lands."
        );
    }

    if yes {
        info!("Proceeding without confirmation (--yes).");
        return Ok(());
    }
    if !stdin().is_terminal() {
        bail!(
            "Refusing to create an order without confirmation in a non-interactive context. \
             Use --yes to proceed."
        );
    }
    let confirmed = Confirm::new()
        .with_prompt("Create this order?")
        .default(false)
        .interact()?;
    if !confirmed {
        bail!("Operation cancelled by user");
    }
    Ok(())
}

/// `10.00 USD -> about 6.818T cycles (9.41 USD after a 0.59 USD card fee, at 1.3520 USD per 1T cycles)`.
fn quote_line(quote: &Quote) -> String {
    format!(
        "{} -> about {} ({} after a {} card fee, at {} per 1T cycles)",
        quote.amount,
        format_cycles(quote.cycles),
        quote.net,
        quote.fee,
        quote.rate.usd_per_trillion()
    )
}

/// Print the Payment Link, and open it where that makes sense: a person at a
/// terminal, who did not ask otherwise. `--json` and `--quiet` are for
/// scripts, which have no browser to open.
fn announce_payable(order: &OrderView, quote: Option<&Quote>, output: Output, no_open: bool) {
    let Some(url) = order.checkout_url.as_deref() else {
        warn!(
            "Order {} is {} but has no Payment Link",
            order.id, order.status
        );
        return;
    };
    match output {
        Output::Json => {
            if let Err(e) = print_json(&JsonOrderCreated {
                order_id: &order.id,
                checkout_url: url,
                amount: order.amount,
                net: order.net,
                fee: quote.map(|q| q.fee),
                estimated_cycles: quote.map(|q| q.cycles.to_string()),
                expires_at: order.expires_at.map(format_secs),
            }) {
                warn!("failed to write JSON: {e}");
            }
        }
        Output::Quiet => println!("{url}"),
        Output::Human => {
            let until = order
                .expires_at
                .map(|secs| format!(", payable until {}", format_secs(secs)))
                .unwrap_or_default();
            info!("Order {}{until}. Pay here:", order.id);
            println!("{url}");
            // A phone camera is the usual way to reach a card from a terminal;
            // the code is for eyes, so it only goes where there are some.
            if stderr().is_terminal()
                && let Some(qr) = render_qr(url)
            {
                eprintln!("{qr}");
            }
            if !no_open && stderr().is_terminal() {
                match open::that(url) {
                    Ok(()) => info!("Opened the Payment Link in your browser."),
                    Err(e) => warn!("Could not open a browser ({e}); open the URL above yourself."),
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn wait_and_report(
    calls: &dyn CanisterCalls,
    agent: &ic_agent::Agent,
    gateway: &Gateway,
    buyer: Principal,
    order: OrderView,
    output: Output,
    debug: bool,
    resume: &str,
) -> Result<(), anyhow::Error> {
    let id = order.id.clone();
    let options = WaitOptions::default();
    let wait = async |reporter: &Reporter| {
        purchase::wait_for_order(
            calls,
            gateway,
            &id,
            Some(order),
            &options,
            stop_signal(),
            reporter,
        )
        .await
    };
    // The live view is for a person; a script gets its answer on stdout.
    let result = match output {
        Output::Human => rendered(debug, wait).await,
        Output::Quiet | Output::Json => wait(&Reporter::null()).await,
    };

    let order = match result {
        Ok(order) => order,
        Err(CyclesPurchaseError::Interrupted { order }) => {
            if output == Output::Human {
                if order.status.is_paid() {
                    info!("Order {} is {}.", order.id, order.status);
                } else {
                    info!("Order {} stays payable{}.", order.id, until(&order));
                }
                info!("Keep waiting:  {resume}");
            }
            bail!("interrupted while waiting for order {}", order.id);
        }
        Err(CyclesPurchaseError::WaitTimedOut { order }) => {
            if output == Output::Human {
                if order.status.is_paid() {
                    info!("Order {} is still {}.", order.id, order.status);
                } else {
                    info!(
                        "No payment arrived. Order {} stays payable{}.",
                        order.id,
                        until(&order)
                    );
                }
                info!("Keep waiting:  {resume}");
            }
            return Err(CyclesPurchaseError::WaitTimedOut { order }.into());
        }
        Err(error) => return Err(error.into()),
    };

    let balance = if order.status == OrderStatus::Delivered {
        match get_raw_balance(agent, CYCLES_LEDGER_PRINCIPAL, buyer, None).await {
            Ok(balance) => balance.0.to_u128(),
            Err(e) => {
                warn!("Cycles were delivered, but the balance could not be read: {e}");
                None
            }
        }
    } else {
        None
    };

    print_final(&order, balance, output)?;

    let detail = order
        .detail
        .as_deref()
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    match order.status {
        OrderStatus::Delivered => {
            if output == Output::Human {
                let cycles = order
                    .cycles_delivered
                    .map(format_cycles)
                    .unwrap_or_else(|| "cycles".to_owned());
                println!("Delivered {cycles} to {buyer}");
                if let Some(balance) = balance {
                    println!("Balance: {}", format_cycles(balance));
                }
            }
            Ok(())
        }
        OrderStatus::Expired => bail!(
            "order {} expired unpaid; a payment made on its page from now on is refunded to \
             the card",
            order.id
        ),
        OrderStatus::Refunding => bail!(
            "order {} could not be delivered and is being refunded to your card{detail}",
            order.id
        ),
        OrderStatus::Refunded => bail!(
            "order {} could not be delivered; {} was refunded to your card{detail}",
            order.id,
            order
                .refunded
                .map(|m| m.to_string())
                .unwrap_or_else(|| "the payment".to_owned())
        ),
        OrderStatus::RefundOwed => bail!(
            "order {} could not be delivered; CyclePay owes you a refund, which an operator \
             makes by hand{detail}. Keep the order id",
            order.id
        ),
        OrderStatus::NeedsReview => bail!(
            "order {} needs review{detail}: your card was charged, and CyclePay checks the \
             order by hand, then delivers or refunds it; keep the order id, and run \
             `{resume}` later to see the outcome",
            order.id
        ),
        OrderStatus::AwaitingPayment | OrderStatus::PaymentPending | OrderStatus::Processing => {
            bail!("order {} is still {}", order.id, order.status)
        }
    }
}

/// ` until 2026-10-13T09:00:00Z`, or nothing when the order has no deadline.
fn until(order: &OrderView) -> String {
    order
        .expires_at
        .map(|secs| format!(" until {}", format_secs(secs)))
        .unwrap_or_default()
}

/// The URL as a QR code drawn with half-block characters, two modules per
/// row, for scanning off the terminal. Inverted (light modules on the
/// terminal's dark background) because that is what most terminals are and
/// scanners read either way. `None` only if the URL is too long to encode.
fn render_qr(url: &str) -> Option<String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    Some(
        code.render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build(),
    )
}

/// Where the order stands, for a script: one JSON object, or the status tag
/// alone. A person is told in prose by the caller.
fn print_final(
    order: &OrderView,
    balance: Option<u128>,
    output: Output,
) -> Result<(), anyhow::Error> {
    match output {
        Output::Json => print_json(&JsonOrderFinal {
            order_id: &order.id,
            status: order.status.tag(),
            cycles_delivered: order.cycles_delivered.map(|c| c.to_string()),
            paid: order.paid,
            refunded: order.refunded,
            detail: order.detail.as_deref(),
            balance: balance.map(|b| b.to_string()),
        }),
        Output::Quiet => {
            println!("{}", order.status.tag());
            Ok(())
        }
        Output::Human => Ok(()),
    }
}

/// What a refusal means for this buyer, with what to do about it.
fn refusal_message(error: &OrderError) -> String {
    match error.kind() {
        OrderErrorKind::AmountOutOfRange => {
            format!("CyclePay refused the amount: {}", error.message)
        }
        // The backend's own messages say what to do; CyclePay allows 5 orders
        // a minute and 50 a UTC day per identity, and a share of all buyers'.
        OrderErrorKind::RateLimited => format!(
            "CyclePay is limiting new orders ({}); it takes 5 a minute and 50 a UTC day from one \
             identity",
            error.message
        ),
        OrderErrorKind::TooManyOpenOrders => {
            format!(
                "this identity has too many unpaid orders: {}",
                error.message
            )
        }
        OrderErrorKind::Paused | OrderErrorKind::CardsUnavailable => {
            format!("CyclePay is not taking card orders: {}", error.message)
        }
        OrderErrorKind::Anonymous => {
            "CyclePay refused the order: the caller is anonymous; choose an identity with \
             --identity"
                .to_owned()
        }
        OrderErrorKind::CanisterIdInvalid
        | OrderErrorKind::DestinationBlocked
        | OrderErrorKind::SystemCanisterWarning
        | OrderErrorKind::BeneficiaryNotAllowed
        | OrderErrorKind::NotProjectMember
        | OrderErrorKind::ProjectsUnavailable
        | OrderErrorKind::Unknown => format!("CyclePay refused the order: {error}"),
    }
}

/// The name behind an identity selection, for the confirmation block. Best
/// effort: a name that cannot be read is simply left out.
async fn identity_name(ctx: &Context, selection: &IdentitySelection) -> Option<String> {
    match selection {
        IdentitySelection::Named(name) => Some(name.clone()),
        IdentitySelection::Default => {
            let dirs = ctx.dirs.identity().ok()?;
            dirs.with_read(async |dirs| IdentityDefaults::load_from(dirs))
                .await
                .ok()?
                .ok()
                .map(|defaults| defaults.default)
        }
        IdentitySelection::Anonymous => None,
    }
}

/// Seconds since the Unix epoch as RFC 3339.
fn format_secs(timestamp: u64) -> String {
    OffsetDateTime::from_unix_timestamp(timestamp as i64)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| timestamp.to_string())
}

/// `2 hours`, `35 minutes`, `3 days`: enough for a warning.
fn format_age(age: std::time::Duration) -> String {
    let secs = age.as_secs();
    let (count, unit) = if secs >= 2 * 86_400 {
        (secs / 86_400, "days")
    } else if secs >= 2 * 3_600 {
        (secs / 3_600, "hours")
    } else {
        (secs / 60, "minutes")
    };
    match (count, unit) {
        (1, "minutes") => "1 minute".to_owned(),
        (count, unit) => format!("{count} {unit}"),
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), anyhow::Error> {
    serde_json::to_writer(stdout(), value)?;
    println!();
    Ok(())
}

#[derive(Serialize)]
struct JsonOrderCreated<'a> {
    order_id: &'a str,
    checkout_url: &'a str,
    /// What the card will be charged, as the order asks.
    amount: Money,
    /// What funds the cycles after the card fee, as the backend set it.
    net: Money,
    /// The card fee included in `amount`. Known only when the order was
    /// created by this run; a resumed order's quote is not kept.
    fee: Option<Money>,
    /// About the cycles `net` buys at the rate quoted, as a decimal string;
    /// likewise only when created by this run.
    estimated_cycles: Option<String>,
    /// RFC 3339.
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct JsonOrderFinal<'a> {
    order_id: &'a str,
    status: &'static str,
    /// Exact cycles delivered, as a decimal string; only when delivered.
    cycles_delivered: Option<String>,
    /// What the card was charged, once paid.
    paid: Option<Money>,
    /// What was refunded, once anything was.
    refunded: Option<Money>,
    /// Why delivery waits or stopped, or why a refund is owed.
    detail: Option<&'a str>,
    /// The buyer's cycles-ledger balance afterwards, as a decimal string.
    balance: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::cyclepay::purchase::Rate;

    fn error(code: &str, message: &str) -> OrderError {
        OrderError {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn refusal_messages_say_what_to_do() {
        // The backend's own messages, as orders_v2.rs words them: nothing
        // they already say is said twice.
        let msg = refusal_message(&error("RATE_LIMITED", "too many orders; try again shortly"));
        assert!(msg.contains("50 a UTC day"), "{msg}");

        let msg = refusal_message(&error(
            "TOO_MANY_OPEN_ORDERS",
            "you have 10 unpaid orders; pay one, or wait for it to expire (25 hours after it \
             was made)",
        ));
        assert_eq!(msg.matches("25 hours").count(), 1, "{msg}");

        let msg = refusal_message(&error("PAUSED", "CyclePay is paused; try again later"));
        assert_eq!(msg.matches("try again later").count(), 1, "{msg}");
        let msg = refusal_message(&error(
            "CARDS_UNAVAILABLE",
            "card payments are not available right now",
        ));
        assert_eq!(msg.matches("right now").count(), 1, "{msg}");

        let msg = refusal_message(&error("SOMETHING_NEW", "as the backend puts it"));
        assert_eq!(
            msg,
            "CyclePay refused the order: SOMETHING_NEW: as the backend puts it"
        );
    }

    #[test]
    fn quote_line_shows_amount_cycles_fee_and_rate() {
        let line = quote_line(&Quote {
            amount: Money::from_minor_units(1000, Currency::Usd),
            fee: Money::from_minor_units(59, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            cycles: 6_959_866_863_905,
            rate: Rate {
                xdr_usd_x10000: 13_520,
                stale: false,
                age: None,
            },
        });
        assert_eq!(
            line,
            "10.00 USD -> about 6.959T cycles (9.41 USD after a 0.59 USD card fee, at 1.3520 USD per 1T cycles)"
        );
    }

    #[test]
    fn qr_renders_a_payment_link() {
        let qr = render_qr(
            "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00?client_reference_id=o_co-1&prefilled_amount=1000",
        )
        .unwrap();
        let lines: Vec<&str> = qr.lines().collect();
        // Square-ish: every row is the same width and the block characters
        // are the only ink.
        let width = lines[0].chars().count();
        assert!(width > 20, "{width}");
        assert!(lines.iter().all(|l| l.chars().count() == width));
        assert!(
            qr.chars()
                .all(|c| matches!(c, '█' | '▀' | '▄' | ' ' | '\n')),
            "{qr}"
        );
        // Far beyond a QR code's capacity.
        assert!(render_qr(&"x".repeat(10_000)).is_none());
    }

    #[test]
    fn past_deadline_reads_the_orders_expiry() {
        let order = |expires_at| OrderView {
            id: "co-1".into(),
            status: OrderStatus::AwaitingPayment,
            amount: Money::from_minor_units(1000, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            checkout_url: None,
            expires_at,
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
        };
        assert!(past_deadline(&order(Some(1_700_000_000))));
        assert!(!past_deadline(&order(Some(u64::MAX / 2))));
        assert!(!past_deadline(&order(None)));
    }

    #[test]
    fn the_resume_command_carries_the_selections() {
        let selections = TokenCommandSelections {
            environment: EnvironmentSelection::Default,
            network: NetworkSelection::Named("ic".into()),
            identity: IdentitySelection::Named("my id".into()),
        };
        assert_eq!(
            selection_flags(&selections, None),
            " -n ic --identity 'my id'"
        );
        let selections = TokenCommandSelections {
            environment: EnvironmentSelection::Named("staging".into()),
            network: NetworkSelection::Default,
            identity: IdentitySelection::Default,
        };
        let gateway = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        assert_eq!(
            selection_flags(&selections, Some(gateway)),
            " -e staging --gateway ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
        assert_eq!(shell_word("co-1-0000abcd"), "co-1-0000abcd");
        assert_eq!(shell_word(""), "''");
    }

    #[test]
    fn timestamps_and_ages_read_well() {
        assert_eq!(format_secs(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_age(Duration::from_secs(90)), "1 minute");
        assert_eq!(format_age(Duration::from_secs(5 * 3600)), "5 hours");
        assert_eq!(format_age(Duration::from_secs(3 * 86_400)), "3 days");
    }
}
