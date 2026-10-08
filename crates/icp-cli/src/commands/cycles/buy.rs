use std::io::{IsTerminal, stderr, stdin, stdout};

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::Args;
use dialoguer::Confirm;
use icp_app::context::Context;
use icp_app::identity::IdentitySelection;
use icp_app::identity::manifest::IdentityDefaults;
use icp_app::operations::cycles_purchase::{
    self as purchase, Currency, CyclesPurchaseError, Money, OrderView, Quote, Target, WaitOptions,
};
use icp_app::operations::token::balance::get_raw_balance;
use icp_app::operations::token::format_cycles;
use icp_canister_interfaces::cycles_gateway::{
    CreateOrderError, OrderStatus, Reason, cycles_gateway_canister_id,
};
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icp_project::calls::CanisterCalls;
use icp_project::parsers::CyclesAmount;
use icp_project::signal::stop_signal;
use num_traits::ToPrimitive;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};

use crate::commands::args::TokenCommandArgs;
use crate::render::{ProgressManager, ProgressManagerSettings};

/// Buy cycles with a card.
///
/// Creates an order on a cycles gateway canister as the current identity,
/// prints the hosted checkout URL (and opens it in a browser when run from a
/// terminal), then waits for the cycles to land on the identity's own
/// cycles-ledger account. Fund a canister from there with `icp canister
/// top-up`. Say either how much to spend (--amount, in the currency given
/// by --currency) or how many cycles to receive (--cycles); the latter is
/// quoted as the least amount that buys them once the card fee is taken out.
/// The gateway accepts USD only today.
///
/// Exactly one of --amount, --cycles, --resume or --cancel must be given.
#[derive(Debug, Args)]
pub(crate) struct BuyArgs {
    /// Amount to spend, as a decimal (e.g. 10 or 12.50), in the currency given by --currency
    #[arg(
        long,
        value_name = "AMOUNT",
        required_unless_present_any = ["cycles", "resume", "cancel"],
        conflicts_with_all = ["cycles", "resume", "cancel"]
    )]
    pub(crate) amount: Option<String>,

    /// Cycles to receive; the amount to pay is worked out from the gateway's quote, fee included.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, value_name = "CYCLES", conflicts_with_all = ["resume", "cancel"])]
    pub(crate) cycles: Option<CyclesAmount>,

    /// ISO 4217 code of the currency to pay in. The gateway accepts USD only.
    #[arg(long, value_name = "CODE", default_value = "USD")]
    pub(crate) currency: Currency,

    /// Continue waiting on an existing order instead of creating a new one
    #[arg(long, value_name = "ORDER_ID", conflicts_with = "cancel")]
    pub(crate) resume: Option<String>,

    /// Cancel an open order
    #[arg(long, value_name = "ORDER_ID")]
    pub(crate) cancel: Option<String>,

    /// Canister id of the cycles gateway to buy through.
    /// Defaults to the mainnet gateway, or to ICP_CYCLES_GATEWAY_CANISTER_ID when set.
    #[arg(long, value_name = "CANISTER_ID")]
    pub(crate) gateway: Option<Principal>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// Skip confirmation prompts
    #[arg(long, short)]
    pub(crate) yes: bool,

    /// Do not open the checkout URL in a browser
    #[arg(long)]
    pub(crate) no_open: bool,

    /// Print the checkout URL and return without waiting for the order to complete
    #[arg(long)]
    pub(crate) no_wait: bool,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the checkout URL, then the final status
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

    let gateway = match args.gateway {
        Some(gateway) => gateway,
        None => cycles_gateway_canister_id().map_err(|message| anyhow!(message))?,
    };
    let output = Output::of(args);

    if let Some(id) = &args.cancel {
        let order = purchase::cancel_order(calls, gateway, id).await?;
        match output {
            Output::Json => print_json(&JsonOrderFinal {
                order_id: &order.id,
                status: order.status.tag(),
                cycles_delivered: None,
                balance: None,
            })?,
            Output::Quiet => println!("{}", order.status.tag()),
            Output::Human => println!("Order {} {}", order.id, order.status),
        }
        return Ok(());
    }

    let order = match &args.resume {
        Some(id) => {
            let order = purchase::get_order(calls, gateway, id).await?;
            if order.status == OrderStatus::Created {
                announce_payable(&order, output, args.no_open);
            } else if output == Output::Human {
                info!("Order {} is {}", order.id, order.status);
            }
            order
        }
        None => {
            let target = target
                .expect("clap requires --amount or --cycles unless --resume or --cancel is given");
            let identity = identity_name(ctx, &selections.identity).await;
            let order =
                create_order(calls, gateway, buyer, identity.as_deref(), target, args).await?;
            announce_payable(&order, output, args.no_open);
            order
        }
    };

    if args.no_wait {
        if output == Output::Human {
            info!(
                "Not waiting. Run `icp cycles buy --resume {}` to wait for delivery.",
                order.id
            );
        }
        return Ok(());
    }

    wait_and_report(calls, &agent, gateway, buyer, order, output).await
}

/// Quote, confirm, create. Re-quotes and re-confirms once if the rate moved
/// against the buyer in between.
async fn create_order(
    calls: &dyn CanisterCalls,
    gateway: Principal,
    buyer: Principal,
    identity: Option<&str>,
    target: Target,
    args: &BuyArgs,
) -> Result<OrderView, anyhow::Error> {
    let mut quote = purchase::quote_target(calls, gateway, target).await?;
    confirm(&quote, target, gateway, buyer, identity, args.yes)?;

    for attempt in 0..2 {
        match purchase::place_order(calls, gateway, &quote).await {
            Ok(order) => return Ok(order),
            Err(CyclesPurchaseError::OrderRefused {
                error: CreateOrderError::QuoteChanged { .. },
            }) if attempt == 0 => {
                quote = purchase::quote_target(calls, gateway, target).await?;
                if args.yes {
                    info!(
                        "The exchange rate moved. New quote: {}. Retrying once.",
                        quote_line(&quote)
                    );
                } else {
                    warn!("The exchange rate moved since the quote.");
                    confirm(&quote, target, gateway, buyer, identity, false)?;
                }
            }
            Err(CyclesPurchaseError::OrderRefused { error }) => {
                bail!("{}", refusal_message(&error, buyer))
            }
            Err(CyclesPurchaseError::OpenOrderExists { order, open, max }) => {
                bail!("{}", open_order_message(&order, open, max))
            }
            Err(error) => return Err(error.into()),
        }
    }
    bail!("the exchange rate is moving too quickly to lock a quote; try again in a minute")
}

/// Show what is about to happen and ask. Without a terminal there is nobody
/// to ask, so `--yes` is required.
fn confirm(
    quote: &Quote,
    target: Target,
    gateway: Principal,
    buyer: Principal,
    identity: Option<&str>,
    yes: bool,
) -> Result<(), anyhow::Error> {
    let identity = identity
        .map(|name| format!(" (identity '{name}')"))
        .unwrap_or_default();
    info!("Buyer:    {buyer}{identity}");
    info!("Gateway:  {gateway} (canister id)");
    if let Target::Receive { cycles, .. } = target {
        info!("Wanted:   at least {}", format_cycles(cycles));
    }
    info!("Quote:    {}", quote_line(quote));

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

fn quote_line(quote: &Quote) -> String {
    format!(
        "{} -> {} (includes a {} card fee; rate locked at order creation)",
        quote.amount,
        format_cycles(quote.cycles),
        quote.fee
    )
}

/// Print the checkout URL, and open it where that makes sense: a person at a
/// terminal, who did not ask otherwise. `--json` and `--quiet` are for
/// scripts, which have no browser to open.
fn announce_payable(order: &OrderView, output: Output, no_open: bool) {
    let Some(url) = order.checkout_url.as_deref() else {
        warn!(
            "Order {} is {} but the gateway reported no checkout URL",
            order.id, order.status
        );
        return;
    };
    match output {
        Output::Json => {
            if let Err(e) = print_json(&JsonOrderCreated {
                order_id: &order.id,
                checkout_url: url,
                locked_cycles: order.locked_cycles.to_string(),
                expires_at: order.expires_at_ns.map(format_ns),
            }) {
                warn!("failed to write JSON: {e}");
            }
        }
        Output::Quiet => println!("{url}"),
        Output::Human => {
            let until = order
                .expires_at_ns
                .map(|ns| format!(", payable until {}", format_ns(ns)))
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
                    Ok(()) => info!("Opened the checkout page in your browser."),
                    Err(e) => warn!("Could not open a browser ({e}); open the URL above yourself."),
                }
            }
        }
    }
}

async fn wait_and_report(
    calls: &dyn CanisterCalls,
    agent: &ic_agent::Agent,
    gateway: Principal,
    buyer: Principal,
    order: OrderView,
    output: Output,
) -> Result<(), anyhow::Error> {
    let progress = ProgressManager::new(ProgressManagerSettings {
        hidden: output != Output::Human,
    });
    let spinner = progress.create_independent_progress_bar();
    spinner.set_message(waiting_message(order.status));

    let id = order.id.clone();
    let result = purchase::wait_for_order(
        calls,
        gateway,
        &id,
        Some(order),
        &WaitOptions::default(),
        stop_signal(),
        &mut |order| spinner.set_message(waiting_message(order.status)),
    )
    .await;
    spinner.finish_and_clear();

    let order = match result {
        Ok(order) => order,
        Err(CyclesPurchaseError::Interrupted { order }) => {
            if output == Output::Human {
                let until = order
                    .expires_at_ns
                    .map(|ns| format!(" until {}", format_ns(ns)))
                    .unwrap_or_default();
                info!("Order {} stays payable{until}.", order.id);
                info!("Keep waiting:  icp cycles buy --resume {}", order.id);
                info!("Cancel it:     icp cycles buy --cancel {}", order.id);
            }
            bail!("interrupted while waiting for order {}", order.id);
        }
        Err(CyclesPurchaseError::WaitTimedOut { order }) => {
            if output == Output::Human {
                info!("Keep waiting:  icp cycles buy --resume {}", order.id);
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

    match output {
        Output::Json => print_json(&JsonOrderFinal {
            order_id: &order.id,
            status: order.status.tag(),
            cycles_delivered: (order.status == OrderStatus::Delivered)
                .then(|| order.locked_cycles.to_string()),
            balance: balance.map(|b| b.to_string()),
        })?,
        Output::Quiet => println!("{}", order.status.tag()),
        Output::Human => {}
    }

    match order.status {
        OrderStatus::Delivered => {
            if output == Output::Human {
                println!(
                    "Delivered {} to {buyer}",
                    format_cycles(order.locked_cycles)
                );
                if let Some(balance) = balance {
                    println!("Balance: {}", format_cycles(balance));
                }
            }
            Ok(())
        }
        OrderStatus::NeedsReview => bail!(
            "order {} needs review: your card was charged, and the gateway operator will \
             resolve delivery. Keep the order id.",
            order.id
        ),
        OrderStatus::Expired => {
            let why = order
                .expired_by
                .map(|by| format!(": {by}"))
                .unwrap_or_default();
            bail!("order {} expired{why}; no charge was made", order.id)
        }
        OrderStatus::Cancelled => bail!("order {} was cancelled; no charge was made", order.id),
        OrderStatus::Abandoned => {
            let why = order
                .abandoned_reason
                .as_deref()
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default();
            bail!("order {} was abandoned by the gateway{why}", order.id)
        }
        OrderStatus::Created | OrderStatus::Paid => {
            bail!("order {} is still {}", order.id, order.status)
        }
    }
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

fn waiting_message(status: OrderStatus) -> String {
    match status {
        OrderStatus::Paid => "Payment received, delivering cycles...".to_owned(),
        _ => "Waiting for payment...".to_owned(),
    }
}

/// What a gateway refusal means for this buyer, with the flag to reach for.
fn refusal_message(error: &CreateOrderError, buyer: Principal) -> String {
    let usd = |cents: &candid::Nat| {
        Money::from_nat_minor_units(cents, Currency::Usd)
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("{cents} USD cents"))
    };
    match error {
        CreateOrderError::NotAdmitted(Reason::AmountBelowMin {
            min_usd_cents,
            usd_cents,
        }) => format!(
            "amount {} is below the gateway minimum of {}",
            usd(usd_cents),
            usd(min_usd_cents)
        ),
        CreateOrderError::NotAdmitted(Reason::AmountAboveMax {
            max_usd_cents,
            usd_cents,
        }) => format!(
            "amount {} is above the gateway maximum of {}",
            usd(usd_cents),
            usd(max_usd_cents)
        ),
        CreateOrderError::NotAdmitted(Reason::TooManyOpenOrders { open, max }) => format!(
            "you already have {open} open order(s) and the gateway allows {max}, but none is \
             visible to this identity; wait for it to expire or run `icp cycles buy --cancel <order-id>`"
        ),
        CreateOrderError::NotAdmitted(Reason::ReserveShort {
            available,
            requested,
        }) => format!(
            "the gateway cannot lock {} right now ({} available); a smaller amount may succeed",
            cycles_text(requested),
            cycles_text(available)
        ),
        CreateOrderError::NotAdmitted(Reason::BuyerNotAllowed) => format!(
            "{buyer} is not on this gateway's allow-list (the gateway is in simulation mode)"
        ),
        CreateOrderError::NotAdmitted(
            reason @ (Reason::CanisterCyclesLow { .. } | Reason::UnboundedGiveaway { .. }),
        ) => gateway_side(&reason.to_string()),
        CreateOrderError::RateUnavailable
        | CreateOrderError::ReserveUnavailable
        | CreateOrderError::SessionUnavailable(_) => gateway_side(&error.to_string()),
        CreateOrderError::CancelledDuringCreation => {
            "the order was cancelled while being created; start over".to_owned()
        }
        CreateOrderError::QuoteChanged { .. } => {
            "the exchange rate is moving too quickly to lock a quote; try again in a minute"
                .to_owned()
        }
        CreateOrderError::Anonymous
        | CreateOrderError::DestinationNotOwned
        | CreateOrderError::IdGeneration
        | CreateOrderError::SimulationScaleTooSmall { .. }
        | CreateOrderError::TierBelowFees(_)
        | CreateOrderError::UnknownTier(_) => {
            format!("the cycles gateway refused the order: {error}")
        }
    }
}

fn gateway_side(detail: &str) -> String {
    format!("the cycles gateway is temporarily unable to take orders ({detail}); try again later")
}

fn cycles_text(cycles: &candid::Nat) -> String {
    cycles
        .0
        .to_u128()
        .map(format_cycles)
        .unwrap_or_else(|| format!("{cycles} cycles"))
}

fn open_order_message(order: &OrderView, open: u128, max: u128) -> String {
    let mut message = format!(
        "you already have an open order {} (status {}; {open} of {max} allowed), so no new order was created.",
        order.id, order.status
    );
    if let Some(url) = &order.checkout_url
        && order.status == OrderStatus::Created
    {
        message.push_str(&format!("\n  Pay it here:   {url}"));
    }
    message.push_str(&format!(
        "\n  Keep waiting:  icp cycles buy --resume {}\n  Cancel it:     icp cycles buy --cancel {}",
        order.id, order.id
    ));
    message
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

fn format_ns(timestamp_ns: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(timestamp_ns as i128)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| timestamp_ns.to_string())
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
    /// Exact cycles promised, as a decimal string.
    locked_cycles: String,
    /// RFC 3339.
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct JsonOrderFinal<'a> {
    order_id: &'a str,
    status: &'static str,
    /// Exact cycles delivered, as a decimal string; only when delivered.
    cycles_delivered: Option<String>,
    /// The buyer's cycles-ledger balance afterwards, as a decimal string.
    balance: Option<String>,
}

#[cfg(test)]
mod tests {
    use candid::Nat;

    use super::*;

    fn buyer() -> Principal {
        Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap()
    }

    #[test]
    fn refusal_messages_state_both_figures() {
        let msg = refusal_message(
            &CreateOrderError::NotAdmitted(Reason::AmountBelowMin {
                min_usd_cents: Nat::from(500u32),
                usd_cents: Nat::from(100u32),
            }),
            buyer(),
        );
        assert_eq!(
            msg,
            "amount 1.00 USD is below the gateway minimum of 5.00 USD"
        );

        let msg = refusal_message(
            &CreateOrderError::NotAdmitted(Reason::ReserveShort {
                available: Nat::from(1_000_000_000_000u64),
                requested: Nat::from(7_238_000_000_000u64),
            }),
            buyer(),
        );
        assert!(
            msg.contains("7.238T cycles") && msg.contains("1T cycles"),
            "{msg}"
        );
    }

    #[test]
    fn gateway_side_refusals_say_try_later() {
        for error in [
            CreateOrderError::RateUnavailable,
            CreateOrderError::ReserveUnavailable,
            CreateOrderError::SessionUnavailable("stripe down".into()),
            CreateOrderError::NotAdmitted(Reason::CanisterCyclesLow {
                balance: Nat::from(1u8),
                min: Nat::from(2u8),
            }),
        ] {
            let msg = refusal_message(&error, buyer());
            assert!(msg.contains("try again later"), "{msg}");
        }
    }

    #[test]
    fn open_order_message_offers_resume_and_cancel() {
        let order = OrderView {
            id: "o1".into(),
            status: OrderStatus::Created,
            locked_cycles: 1,
            checkout_url: Some("https://checkout.example/o1".into()),
            expires_at_ns: None,
            paid: None,
            expired_by: None,
            abandoned_reason: None,
        };
        let msg = open_order_message(&order, 1, 1);
        assert!(msg.contains("https://checkout.example/o1"), "{msg}");
        assert!(msg.contains("--resume o1"), "{msg}");
        assert!(msg.contains("--cancel o1"), "{msg}");
    }

    #[test]
    fn quote_line_shows_amount_cycles_and_fee() {
        let line = quote_line(&Quote {
            amount: Money::from_minor_units(1000, Currency::Usd),
            fee: Money::from_minor_units(59, Currency::Usd),
            cycles: 7_238_000_000_000,
            minimum_cycles: 0,
        });
        assert_eq!(
            line,
            "10.00 USD -> 7.238T cycles (includes a 0.59 USD card fee; rate locked at order creation)"
        );
    }

    #[test]
    fn qr_renders_a_checkout_url() {
        let qr =
            render_qr("https://checkout.stripe.com/c/pay/cs_test_a1B2c3D4e5F6g7H8i9J0").unwrap();
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
    fn format_ns_is_rfc3339() {
        assert_eq!(format_ns(1_700_000_000_000_000_000), "2023-11-14T22:13:20Z");
    }
}
