use std::io::{self, IsTerminal, stderr, stdin, stdout};
use std::time::Duration;

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::Args;
use dialoguer::Confirm;
use dialoguer::console::Term;
use icp_app::context::{Context, NetworkSelection};
use icp_app::identity::IdentitySelection;
use icp_app::identity::manifest::IdentityDefaults;
use icp_app::operations::token::balance::get_raw_balance;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icp_events::EventKind;
use icp_project::calls::CanisterCalls;
use icp_project::host::EnvironmentSelection;
use icp_project::network::RootKeySpec;
use icp_project::operations::task::{
    CyclesPurchaseTask, Presentation, PurchasePhase, Reporter, Task,
};
use icp_project::parsers::CyclesAmount;
use icp_project::signal::stop_signal;
use num_traits::ToPrimitive;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};

use crate::Stopped;
use crate::commands::args::{TokenCommandArgs, TokenCommandSelections};
use crate::cyclepay::format_cycles;
use crate::cyclepay::interface::{
    CYCLES_GATEWAY_CID, OrderError, OrderErrorKind, cycles_gateway_canister_id, payment_link_url,
};
use crate::cyclepay::money::{Currency, Money};
use crate::cyclepay::purchase::{
    self as purchase, CyclesPurchaseError, Gateway, OrderStatus, OrderView, Quote, Rate, Target,
    WaitOptions, now_secs,
};
use crate::render::rendered;

/// Buy cycles with a card.
///
/// Creates an order on the CyclePay backend canister as the current identity,
/// prints the Payment Link to pay on (and opens it in a browser, except on CI
/// or over SSH), then waits for the cycles to land on the identity's own
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

    // Only the identity that made an order can read it, on the network its
    // gateway is on: the command that resumes it says both.
    let resume = |id: &str| {
        format!(
            "icp cycles buy --resume {}{}",
            shell_word(id),
            selection_flags(&selections, args.gateway)
        )
    };

    // Whether the Payment Link was handed out: an order that is payable right
    // now. Anything else has nothing to pay, and `--no-wait` must still say
    // where it stands.
    let (order, payable, interrupt) = match &args.resume {
        Some(id) => {
            let order = purchase::get_order(calls, &gateway, id)
                .await?
                .at(now_secs());
            // A settled order, an expired one included, is told as a wait
            // that ended so: the same output and exit status, with or
            // without --no-wait.
            if order.status.is_terminal() {
                return report_end(&agent, buyer, &order, output, &resume(&order.id)).await;
            }
            let payable = order.status == OrderStatus::AwaitingPayment;
            if payable {
                announce_payable(&order, None, output, args.no_open);
            } else if output == Output::Human {
                info!("Order {} is {}", order.id, order.status);
            }
            (order, payable, Interrupt::later())
        }
        None => {
            let target =
                target.expect("clap requires --amount or --cycles unless --resume is given");
            let identity = identity_name(ctx, &selections.identity).await;
            let (order, quote, interrupt) =
                create_order(calls, &gateway, buyer, identity.as_deref(), target, args).await?;
            announce_payable(&order, Some(&quote), output, args.no_open);
            (order, true, interrupt)
        }
    };

    if args.no_wait {
        if !payable {
            print_final(&order, None, output)?;
        }
        if output == Output::Human && !order.status.is_terminal() {
            info!("Not waiting. Wait for delivery later with:");
            info!("  {}", resume(&order.id));
        }
        return Ok(());
    }

    let resume = resume(&order.id);
    wait_and_report(
        calls, &agent, &gateway, buyer, order, interrupt, output, ctx.debug, &resume,
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

/// Quote, confirm, create. Returns the order with the quote it was placed on,
/// and what its wait is to stop on.
async fn create_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
    target: Target,
    args: &BuyArgs,
) -> Result<(OrderView, Quote, Interrupt), anyhow::Error> {
    let quote = purchase::quote_target(calls, gateway, target).await?;
    // A script that said --yes reads the figures off --json; -q asks for
    // nothing but the link.
    let speak = !args.yes || !(args.quiet || args.json);
    let interrupt = confirm(&quote, target, gateway, buyer, identity, args.yes, speak)?;
    match purchase::place_order(calls, gateway, quote.amount).await {
        Ok(order) => Ok((order, quote, interrupt)),
        Err(CyclesPurchaseError::OrderRefused { error }) => {
            bail!("{}", refusal_message(&error))
        }
        Err(error) => Err(error.into()),
    }
}

/// Show what is about to happen and ask. Without a terminal there is nobody
/// to ask, so `--yes` is required. Returns what the wait is to stop on.
#[allow(clippy::too_many_arguments)]
fn confirm(
    quote: &Quote,
    target: Target,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
    yes: bool,
    speak: bool,
) -> Result<Interrupt, anyhow::Error> {
    if speak {
        let identity = identity
            .map(|name| format!(" (identity '{name}')"))
            .unwrap_or_default();
        info!("Buyer:    {buyer}{identity}");
        info!("Gateway:  {} (CyclePay backend canister)", gateway.canister);
        if let Target::Receive { cycles, .. } = target {
            info!("Wanted:   at least {}", format_cycles(cycles));
        }
        info!("Quote:    {}", quote_line(quote));
    }
    // Said even when the quote is not: a script that reads the figures off
    // --json, or -q's, would otherwise never hear they may be off.
    if let Some(warning) = stale_rate_warning(&quote.rate) {
        warn!("{warning}");
    }
    if !speak {
        return Ok(Interrupt::later());
    }

    if yes {
        info!("Proceeding without confirmation (--yes).");
        return Ok(Interrupt::later());
    }
    if !stdin().is_terminal() {
        bail!(
            "Refusing to create an order without confirmation in a non-interactive context. \
             Use --yes to proceed."
        );
    }
    let interrupt = Interrupt::catch_at_prompt()?;
    let confirmed = Confirm::new()
        .with_prompt("Create this order?")
        .default(false)
        .interact();
    match confirmed {
        Ok(true) => Ok(interrupt),
        Ok(false) => bail!("Operation cancelled by user"),
        // Ctrl-C, read as an answer: the prompt leaves its cursor hidden
        // and its line open.
        Err(dialoguer::Error::IO(e)) if e.kind() == io::ErrorKind::Interrupted => {
            let _ = Term::stderr().show_cursor();
            eprintln!();
            info!("No order created.");
            Err(Stopped::INTERRUPTED.into())
        }
        Err(e) => Err(e.into()),
    }
}

/// Ctrl-C, or a stop signal, for the wait to stop on.
///
/// Catching Ctrl-C is what lets the prompt read it as an answer (left alone,
/// it ends the process with the prompt's cursor hidden), and once caught it
/// stays caught for the life of the process. So a prompt that catches it
/// hands the same listener on to the wait, which then stops at once for a
/// Ctrl-C pressed while the order was being made, rather than it being lost.
struct Interrupt {
    #[cfg(unix)]
    sigint: Option<tokio::signal::unix::Signal>,
}

impl Interrupt {
    /// Nothing caught yet: the wait catches its own.
    fn later() -> Self {
        Self {
            #[cfg(unix)]
            sigint: None,
        }
    }

    /// Catch Ctrl-C from now on, for the prompt to read it. Only on Unix:
    /// Windows hands a caught Ctrl-C to no reader, so the prompt there is
    /// left as it is.
    fn catch_at_prompt() -> io::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            sigint: Some(tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::interrupt(),
            )?),
        })
    }

    async fn received(self) {
        #[cfg(unix)]
        if let Some(mut sigint) = self.sigint {
            tokio::select! {
                _ = sigint.recv() => {}
                _ = stop_signal() => {}
            }
            return;
        }
        stop_signal().await
    }
}

/// While it lives, the terminal does not echo Ctrl-C as `^C`. The echo lands
/// after the live view's line, and a line that fills the width wraps it onto
/// a row the view does not know about, which then keeps a stale copy of the
/// line when the view is cleared.
struct QuietCtrlC {
    #[cfg(unix)]
    saved: Option<nix::sys::termios::Termios>,
}

impl QuietCtrlC {
    fn new() -> Self {
        #[cfg(unix)]
        {
            use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
            let saved = tcgetattr(stderr())
                .ok()
                .filter(|termios| termios.local_flags.contains(LocalFlags::ECHOCTL));
            let quieted = saved.as_ref().is_some_and(|saved| {
                let mut quiet = saved.clone();
                quiet.local_flags.remove(LocalFlags::ECHOCTL);
                tcsetattr(stderr(), SetArg::TCSANOW, &quiet).is_ok()
            });
            Self {
                saved: saved.filter(|_| quieted),
            }
        }
        #[cfg(not(unix))]
        Self {}
    }
}

impl Drop for QuietCtrlC {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(saved) = &self.saved {
            use nix::sys::termios::{SetArg, tcsetattr};
            let _ = tcsetattr(stderr(), SetArg::TCSANOW, saved);
        }
    }
}

/// What to say of a rate CyclePay has not refreshed for longer than it
/// tolerates, if it has not.
fn stale_rate_warning(rate: &Rate) -> Option<String> {
    let age = rate
        .age
        .map(|age| format!(" for {}", format_age(age)))
        .unwrap_or_default();
    rate.stale.then(|| {
        format!(
            "CyclePay has not refreshed its exchange rate{age}; the quote may be off, and the \
             cycles delivered follow the rate when the payment lands."
        )
    })
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

/// Print the Payment Link, and open it where that makes sense: for a person
/// who did not ask otherwise, on a machine they are at (an agent running the
/// command for them included). `--json` and `--quiet` are for scripts, which
/// have no browser to open.
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
            let color = std::env::var_os("NO_COLOR").is_none();
            if stderr().is_terminal()
                && let Some(qr) = render_qr(url, color)
            {
                // A code wrapped across lines cannot be scanned.
                let (_, columns) = dialoguer::console::Term::stderr().size();
                let width = qr
                    .lines()
                    .next()
                    .map_or(0, |row| row.chars().filter(|c| "█▀▄ ".contains(*c)).count());
                if width > usize::from(columns) {
                    info!("(Widen the terminal to {width} columns to see the QR code.)");
                } else {
                    eprintln!("{qr}");
                }
            }
            if !no_open && browser_is_at_hand() {
                match open::that(url) {
                    Ok(()) => info!("Opened the Payment Link in your browser."),
                    Err(e) => warn!("Could not open a browser ({e}); open the URL above yourself."),
                }
            }
        }
    }
}

/// Whether a browser opened here would be in front of the buyer: not on a
/// CI runner, nor at the far end of an SSH session.
fn browser_is_at_hand() -> bool {
    let set = |name| std::env::var(name).is_ok_and(|v| !matches!(v.as_str(), "" | "0" | "false"));
    !(set("CI") || set("SSH_CONNECTION") || set("SSH_CLIENT") || set("SSH_TTY"))
}

#[allow(clippy::too_many_arguments)]
async fn wait_and_report(
    calls: &dyn CanisterCalls,
    agent: &ic_agent::Agent,
    gateway: &Gateway,
    buyer: Principal,
    order: OrderView,
    interrupt: Interrupt,
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
            interrupt.received(),
            reporter,
        )
        .await
    };
    // The live view is for a person at a terminal. Anyone else reading
    // stderr as it comes (an agent, a log) is told in lines instead; -q
    // asks for nothing but the link and the end.
    let result = match output {
        Output::Human if stderr().is_terminal() => {
            let quiet = QuietCtrlC::new();
            let result = rendered(debug, wait).await;
            drop(quiet);
            // On a terminal the live view leaves its last line open (as it
            // does for `icp deploy`), so what follows would run on from it.
            if !debug {
                eprintln!();
            }
            result
        }
        Output::Human | Output::Json => {
            info!("If this command is stopped, keep waiting with:");
            info!("  {resume}");
            with_heartbeat(HEARTBEAT, |line| info!("{line}"), wait).await
        }
        Output::Quiet => wait(&Reporter::null()).await,
    };

    let order = match result {
        Ok(order) => order,
        // Stopped, not failed: the order carries on without the command.
        Err(CyclesPurchaseError::Interrupted { order }) => {
            if output != Output::Quiet {
                info!("{}", standing(&order, false));
                info!("Keep waiting with:");
                info!("  {resume}");
            }
            return Err(Stopped::INTERRUPTED.into());
        }
        Err(CyclesPurchaseError::WaitTimedOut { order }) => {
            if output != Output::Quiet {
                info!("{}", standing(&order, true));
                info!("Keep waiting with:");
                info!("  {resume}");
            }
            return Err(CyclesPurchaseError::WaitTimedOut { order }.into());
        }
        Err(error) => return Err(error.into()),
    };

    report_end(agent, buyer, &order, output, resume).await
}

/// How often a wait with nothing drawn says it is still going.
const HEARTBEAT: Duration = Duration::from_secs(60);

/// Run a wait with no live view, saying when each of its phases begins and,
/// every `period`, that it is still going: off a terminal nothing is drawn,
/// and whoever reads the output as it comes would otherwise hear nothing
/// until the order settles, up to an hour later.
async fn with_heartbeat<T>(
    period: Duration,
    say: impl Fn(String),
    op: impl AsyncFnOnce(&Reporter) -> T,
) -> T {
    let (reporter, mut events) = icp_events::channel();
    let op = async move {
        let result = op(&reporter).await;
        drop(reporter);
        result
    };
    let beat = async {
        let started = tokio::time::Instant::now();
        let mut ticks = tokio::time::interval_at(started + period, period);
        let mut phase: Option<CyclesPurchaseTask> = None;
        loop {
            tokio::select! {
                event = events.recv() => match event.map(|event| event.kind) {
                    None => break,
                    Some(EventKind::TaskStarted {
                        task: Task::CyclesPurchase(task),
                        ..
                    }) => {
                        if let Some(message) = task.running_message() {
                            say(format!("[order {}] {message}", task.order_id));
                        }
                        phase = Some(task);
                    }
                    Some(_) => {}
                },
                _ = ticks.tick() => {
                    if let Some(task) = &phase {
                        let doing = match task.phase {
                            PurchasePhase::AwaitingPayment => "Still waiting for payment",
                            PurchasePhase::Delivering => "Payment received; still delivering cycles",
                        };
                        say(format!(
                            "[order {}] {doing} ({} so far)",
                            task.order_id,
                            format_age(started.elapsed())
                        ));
                    }
                }
            }
        }
    };
    let (result, ()) = tokio::join!(op, beat);
    result
}

/// Tell how a settled order ended, and fail unless it was delivered. Said
/// the same way after a wait and for an order resumed already settled.
async fn report_end(
    agent: &ic_agent::Agent,
    buyer: Principal,
    order: &OrderView,
    output: Output,
    resume: &str,
) -> Result<(), anyhow::Error> {
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

    print_final(order, balance, output)?;

    if order.status == OrderStatus::Delivered && output == Output::Human {
        let cycles = order
            .cycles_delivered
            .map(format_cycles)
            .unwrap_or_else(|| "cycles".to_owned());
        // The status is read by query, so it is CyclePay's report as one
        // replica gives it, and is said to be.
        println!("CyclePay reports {cycles} delivered to {buyer}");
        if let Some(balance) = balance {
            println!("Balance: {}", format_cycles(balance));
        }
    }
    ending(order, resume)
}

/// How a settled order ended, as the command's result: `Ok` for a delivery,
/// and an error that says what happened otherwise.
fn ending(order: &OrderView, resume: &str) -> Result<(), anyhow::Error> {
    let detail = order
        .detail
        .as_deref()
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    match order.status {
        OrderStatus::Delivered => Ok(()),
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
        // Not a promise of a refund: a payment disputed before delivery
        // also needs review, and the dispute settles it.
        OrderStatus::NeedsReview => bail!(
            "order {} needs review{detail}: your card was charged, and CyclePay checks the \
             order by hand (a disputed payment is settled by the dispute). Keep the order id, \
             and run `{resume}` later to see the outcome",
            order.id
        ),
        OrderStatus::AwaitingPayment
        | OrderStatus::PaymentPending
        | OrderStatus::Processing
        | OrderStatus::Unknown => bail!("order {} is still {}", order.id, order.status),
    }
}

/// Where an order the wait left unsettled stands, when it was stopped or
/// (`timed_out`) gave up. Only an order known to be unpaid is payable.
fn standing(order: &OrderView, timed_out: bool) -> String {
    match order.status {
        OrderStatus::AwaitingPayment if timed_out => format!(
            "No payment arrived. Order {} stays payable{}.",
            order.id,
            until(order)
        ),
        OrderStatus::AwaitingPayment => {
            format!("Order {} stays payable{}.", order.id, until(order))
        }
        status if timed_out => format!("Order {} is still {status}.", order.id),
        status => format!("Order {} is {status}.", order.id),
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
/// row, for scanning off the terminal. It takes the lowest error correction
/// (a screen does not smudge) and a two-module quiet zone, the least a
/// scanner needs, so that the Payment Link's code is 41x21 and fits an 80x24
/// terminal. Light modules are the blocks; with `color`, the code is set
/// white on black so that it reads the same on a light theme as on a dark
/// one. `None` only if the URL is too long to encode.
fn render_qr(url: &str, color: bool) -> Option<String> {
    use itertools::Itertools;
    use qrcode::{Color, EcLevel, QrCode};
    const QUIET: usize = 2;
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L).ok()?;
    let width = code.width();
    let modules = code.to_colors();
    let size = width + 2 * QUIET;
    // The quiet zone, and the half row below an odd height, are light.
    let light = |x: usize, y: usize| {
        let inside = (QUIET..QUIET + width).contains(&x) && (QUIET..QUIET + width).contains(&y);
        !inside || modules[(y - QUIET) * width + (x - QUIET)] == Color::Light
    };
    let (on, off) = if color {
        ("\x1b[97;40m", "\x1b[0m")
    } else {
        ("", "")
    };
    let mut rows = (0..size).step_by(2).map(|y| {
        let row: String = (0..size)
            .map(|x| match (light(x, y), light(x, y + 1)) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            })
            .collect();
        format!("{on}{row}{off}")
    });
    Some(rows.join("\n"))
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
        // The backend's own messages say what to do, and whether the limit
        // is the identity's or one on every buyer: both send these codes.
        OrderErrorKind::RateLimited => {
            format!("CyclePay is limiting new orders: {}", error.message)
        }
        OrderErrorKind::TooManyOpenOrders => {
            format!("CyclePay refused the order: {}", error.message)
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
fn format_age(age: Duration) -> String {
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
    use icp_events::TaskOutcome;

    use super::*;

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
        // Each of these codes is sent both for the identity's own limit and
        // for one on every buyer, so neither is put down to the identity.
        for message in [
            "too many orders; try again shortly",
            "too many new orders this minute; try again shortly",
        ] {
            let msg = refusal_message(&error("RATE_LIMITED", message));
            assert_eq!(msg, format!("CyclePay is limiting new orders: {message}"));
        }
        for message in [
            "you have 10 unpaid orders; pay one, or wait for it to expire (25 hours after it was \
             made)",
            "too many unpaid orders right now; try again later",
        ] {
            let msg = refusal_message(&error("TOO_MANY_OPEN_ORDERS", message));
            assert_eq!(msg, format!("CyclePay refused the order: {message}"));
        }

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
        let url = "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00?client_reference_id=o_co-1-0000abcd&prefilled_amount=1000";
        let qr = render_qr(url, false).unwrap();
        let lines: Vec<&str> = qr.lines().collect();
        // Square-ish: every row is the same width and the block characters
        // are the only ink.
        let width = lines[0].chars().count();
        assert!(width > 20 && width <= 45, "{width}");
        assert!(lines.len() <= 23, "{}", lines.len());
        assert!(lines.iter().all(|l| l.chars().count() == width));
        assert!(
            qr.chars()
                .all(|c| matches!(c, '█' | '▀' | '▄' | ' ' | '\n')),
            "{qr}"
        );
        // In colour, every row is set white on black and reset after.
        let colored = render_qr(url, true).unwrap();
        assert!(
            colored
                .lines()
                .all(|l| l.starts_with("\x1b[97;40m") && l.ends_with("\x1b[0m")),
            "{colored}"
        );
        // Far beyond a QR code's capacity.
        assert!(render_qr(&"x".repeat(10_000), false).is_none());
    }

    /// Read back, the drawing is the code's own modules inside a light
    /// quiet zone two modules wide, with the half row below its odd height
    /// light too: a dark stripe there would leave a scanner no edge to find.
    #[test]
    fn qr_draws_every_module_where_it_is() {
        use qrcode::{Color, EcLevel, QrCode};
        let url = "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00?client_reference_id=o_co-4-77fd4ac0&prefilled_amount=100";
        let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L).unwrap();
        let (width, modules) = (code.width(), code.to_colors());
        let rows: Vec<Vec<char>> = render_qr(url, false)
            .unwrap()
            .lines()
            .map(|l| l.chars().collect())
            .collect();
        let size = width + 4;
        assert_eq!(rows.len(), size.div_ceil(2));
        assert!(rows.iter().all(|row| row.len() == size));
        for y in 0..rows.len() * 2 {
            for x in 0..size {
                let drawn_light = match (rows[y / 2][x], y % 2) {
                    ('█', _) | ('▀', 0) | ('▄', 1) => true,
                    (' ', _) | ('▀', 1) | ('▄', 0) => false,
                    (c, _) => panic!("unexpected {c:?}"),
                };
                let inside = (2..2 + width).contains(&x) && (2..2 + width).contains(&y);
                let light = !inside || modules[(y - 2) * width + (x - 2)] == Color::Light;
                assert_eq!(drawn_light, light, "module ({x}, {y})");
            }
        }
    }

    /// Only a delivery succeeds; every other end is an error that says what
    /// happened, whether it is reported after a wait or for an order resumed
    /// already settled.
    #[test]
    fn only_a_delivery_ends_well() {
        let order = |status| OrderView {
            id: "co-1".into(),
            status,
            amount: Money::from_minor_units(1000, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            checkout_url: None,
            expires_at: None,
            paid: Some(Money::from_minor_units(1000, Currency::Usd)),
            cycles_delivered: None,
            refunded: Some(Money::from_minor_units(1000, Currency::Usd)),
            detail: None,
        };
        let resume = "icp cycles buy --resume co-1 -n ic";
        assert!(ending(&order(OrderStatus::Delivered), resume).is_ok());
        for (status, says) in [
            (OrderStatus::Refunded, "10.00 USD was refunded"),
            (OrderStatus::Refunding, "is being refunded"),
            (OrderStatus::RefundOwed, "owes you a refund"),
            (OrderStatus::NeedsReview, resume),
            (OrderStatus::Expired, "expired unpaid"),
        ] {
            let err = ending(&order(status), resume).unwrap_err().to_string();
            assert!(err.contains(says), "{status:?}: {err}");
        }
    }

    /// A wait that stops or gives up calls an order payable only when it is
    /// known to be unpaid.
    #[test]
    fn an_unsettled_order_is_payable_only_when_known_unpaid() {
        let order = |status| OrderView {
            id: "co-1".into(),
            status,
            amount: Money::from_minor_units(1000, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            checkout_url: None,
            expires_at: Some(1_700_000_000),
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
        };
        assert_eq!(
            standing(&order(OrderStatus::AwaitingPayment), false),
            "Order co-1 stays payable until 2023-11-14T22:13:20Z."
        );
        assert_eq!(
            standing(&order(OrderStatus::AwaitingPayment), true),
            "No payment arrived. Order co-1 stays payable until 2023-11-14T22:13:20Z."
        );
        assert_eq!(
            standing(&order(OrderStatus::Processing), true),
            "Order co-1 is still paid, delivering."
        );
        for timed_out in [false, true] {
            let said = standing(&order(OrderStatus::Unknown), timed_out);
            assert!(said.contains("does not know"), "{said}");
            assert!(
                !said.contains("payable") && !said.contains("No payment"),
                "{said}"
            );
        }
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

    /// Off a terminal, a wait says when each phase begins and keeps saying
    /// that it goes on, in the order things happen, until it ends.
    #[tokio::test]
    async fn a_wait_with_nothing_drawn_keeps_saying_it_goes_on() {
        let said = std::sync::Mutex::new(Vec::new());
        let period = Duration::from_millis(20);
        let ended = with_heartbeat(
            period,
            |line| said.lock().unwrap().push(line),
            async |reporter: &Reporter| {
                for phase in [PurchasePhase::AwaitingPayment, PurchasePhase::Delivering] {
                    let task = reporter.task(Task::cycles_purchase("co-1", phase));
                    tokio::time::sleep(period * 5).await;
                    task.finish(TaskOutcome::succeeded());
                }
                "delivered"
            },
        )
        .await;
        assert_eq!(ended, "delivered");
        let said = said.into_inner().unwrap();
        let first = |text: &str| {
            said.iter()
                .position(|line| line.contains(text))
                .unwrap_or_else(|| panic!("no {text:?} in {said:?}"))
        };
        assert_eq!(said[0], "[order co-1] Waiting for payment...");
        let delivering = first("[order co-1] Payment received; delivering cycles...");
        assert!(
            first("Still waiting for payment (") < delivering,
            "{said:?}"
        );
        assert!(first("still delivering cycles (") > delivering, "{said:?}");
    }

    #[test]
    fn only_a_stale_rate_is_warned_of() {
        let rate = |stale| Rate {
            xdr_usd_x10000: 13_520,
            stale,
            age: Some(Duration::from_secs(5 * 3600)),
        };
        assert_eq!(stale_rate_warning(&rate(false)), None);
        let warning = stale_rate_warning(&rate(true)).unwrap();
        assert!(warning.contains("for 5 hours"), "{warning}");
    }

    #[test]
    fn timestamps_and_ages_read_well() {
        assert_eq!(format_secs(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_age(Duration::from_secs(90)), "1 minute");
        assert_eq!(format_age(Duration::from_secs(5 * 3600)), "5 hours");
        assert_eq!(format_age(Duration::from_secs(3 * 86_400)), "3 days");
    }
}
