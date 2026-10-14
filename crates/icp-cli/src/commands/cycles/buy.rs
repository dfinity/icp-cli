use std::io::{self, IsTerminal, stderr, stdin, stdout};
use std::time::Duration;

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::Args;
use dialoguer::Confirm;
use dialoguer::console::Term;
use icp_app::context::{Context, GetAgentError, GetAgentForEnvError, NetworkSelection};
use icp_app::identity::IdentitySelection;
use icp_app::identity::manifest::IdentityDefaults;
use icp_app::operations::token::balance::get_raw_balance;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icp_events::{EventKind, TaskOutcome};
use icp_project::calls::CanisterCalls;
use icp_project::host::EnvironmentSelection;
use icp_project::parsers::CyclesAmount;
use icp_project::signal::stop_signal;
use num_traits::ToPrimitive;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};

use crate::Stopped;
use crate::commands::args::TokenCommandArgs;
use crate::cyclepay::format_cycles;
use crate::cyclepay::interface::{
    CYCLES_GATEWAY_CID, Destination, OrderError, OrderErrorKind, cycles_gateway_canister_id,
};
use crate::cyclepay::money::{Currency, Money};
use crate::cyclepay::purchase::{
    self as purchase, CyclesPurchaseError, Gateway, MAINNET_HINT, OrderStatus, OrderView, Quote,
    Rate, Target, WaitOptions, now_secs,
};
use crate::cyclepay::task::{CyclesPurchaseTask, PurchasePhase, Reporter};
use crate::render::rendered;

/// Buy cycles with a card.
///
/// Creates an order on the CyclePay backend canister as the current identity,
/// prints the Payment Link to pay on (and opens it in a browser when run from
/// a terminal, but not over SSH or on CI), then waits for the cycles to land
/// on the identity's own cycles-ledger account. Fund a canister from there
/// with `icp canister top-up`. Say either how much to spend (--amount, in the
/// currency given by --currency) or how many cycles to receive (--cycles); the
/// latter pays the least amount that buys them once the card fee is taken
/// out. The gateway publishes its per-order range and its fees, and the quote
/// shows them. CyclePay accepts USD only, from 1 to 200 per order.
///
/// Exactly one of --amount or --cycles must be given.
#[derive(Debug, Args)]
pub(crate) struct BuyArgs {
    /// Amount to spend, as a decimal (e.g. 10 or 12.50), in the currency given by --currency
    #[arg(
        long,
        value_name = "AMOUNT",
        required_unless_present = "cycles",
        conflicts_with = "cycles"
    )]
    pub(crate) amount: Option<String>,

    /// Cycles to receive; the amount to pay is worked out from the current rate, card fee included.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, value_name = "CYCLES")]
    pub(crate) cycles: Option<CyclesAmount>,

    /// ISO 4217 code of the currency to pay in. CyclePay accepts USD only.
    #[arg(long, value_name = "CODE", default_value = "USD")]
    pub(crate) currency: Currency,

    /// Canister id of the cycles gateway to buy through: the CyclePay backend, or any canister
    /// serving its order interface. Defaults to CyclePay's mainnet backend, or to
    /// ICP_CYCLES_GATEWAY_CANISTER_ID when set.
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
        (Some(amount), _) => Target::Spend(Money::parse_decimal(amount, args.currency)?),
        (None, Some(cycles)) => {
            if cycles.get() == 0 {
                bail!("--cycles must be greater than zero");
            }
            Target::Receive {
                cycles: cycles.get(),
                currency: args.currency,
            }
        }
        (None, None) => bail!("one of --amount or --cycles is required"),
    };

    let selections = args.token_command_args.selections();
    if matches!(selections.identity, IdentitySelection::Anonymous) {
        bail!("cycles cannot be bought as the anonymous identity; choose one with --identity");
    }

    let canister = match args.gateway {
        Some(gateway) => gateway,
        None => cycles_gateway_canister_id().map_err(|message| anyhow!(message))?,
    };
    let cyclepay = canister.to_text() == CYCLES_GATEWAY_CID;

    let agent = match ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await
    {
        Ok(agent) => agent,
        // Named neither, the network is a project's own local one, or none
        // outside a project; CyclePay's gateway is on mainnet only.
        Err(
            error @ (GetAgentError::NoProjectOrNetwork
            | GetAgentError::GetAgentForEnv {
                source: GetAgentForEnvError::NetworkAccess { .. },
            }),
        ) if cyclepay
            && matches!(selections.network, NetworkSelection::Default)
            && matches!(selections.environment, EnvironmentSelection::Default) =>
        {
            bail!("{error}. {MAINNET_HINT}")
        }
        Err(error) => return Err(error.into()),
    };
    let buyer = agent
        .get_principal()
        .map_err(|message| anyhow!("failed to get the identity's principal: {message}"))?;
    if buyer == Principal::anonymous() {
        bail!("cycles cannot be bought as the anonymous identity; choose one with --identity");
    }
    let calls = icp_app::calls::calls(agent.clone(), None)?;
    let calls = calls.as_ref();
    // How the gateway sells: the terms every quote and every link below are
    // made on. A gateway that is not on this network says so here.
    let gateway = purchase::connect(calls, canister).await?;
    let output = Output::of(args);

    let identity = identity_name(ctx, &selections.identity).await;
    let (order, quote, mut interrupt) =
        create_order(calls, &gateway, buyer, identity.as_deref(), target, args).await?;
    // Ctrl-C while the order was being made: it exists now, so say where it
    // is, but open nothing and wait for nothing.
    if interrupt.pressed() {
        // The terminal echoed it as `^C` with no line ending.
        if stderr().is_terminal() {
            eprintln!();
        }
        announce_payable(&order, &quote, output, true);
        if output != Output::Quiet {
            info!("Stopped before waiting. {AFTER_HINT}");
        }
        return Err(Stopped::INTERRUPTED.into());
    }
    announce_payable(&order, &quote, output, args.no_open);

    if args.no_wait {
        if output == Output::Human {
            info!("Not waiting. {AFTER_HINT}");
        }
        return Ok(());
    }

    wait_and_report(
        calls, &agent, &gateway, buyer, order, interrupt, output, ctx.debug,
    )
    .await
}

/// What becomes of an order the command is no longer waiting on. The order
/// needs nothing from the command: the gateway delivers once the payment
/// lands, so the buyer is told where the cycles will show.
const AFTER_HINT: &str = "Once the order is paid, the cycles land on your identity's \
                          cycles-ledger account; `icp cycles balance` shows them.";

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
        for line in quote_block(quote, target, gateway, buyer, identity) {
            info!("{line}");
        }
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
        .with_prompt("Create this order? Nothing is charged until you pay on Stripe.")
        .default(false)
        .interact();
    match confirmed {
        Ok(true) => Ok(interrupt),
        // Said as Ctrl-C there is: the same line, and the same status.
        Ok(false) => {
            info!("No order created.");
            Err(Stopped::DECLINED.into())
        }
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
/// keeps the listener, and a Ctrl-C pressed while the order was being made is
/// not lost: the command stops as soon as the order exists, before opening a
/// browser or waiting.
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

    /// Whether Ctrl-C has been pressed since it was caught, without waiting
    /// for one.
    fn pressed(&mut self) -> bool {
        #[cfg(unix)]
        if let Some(sigint) = &mut self.sigint {
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            return sigint.poll_recv(&mut cx).is_ready();
        }
        false
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

/// While it lives, the terminal echoes nothing typed: not Ctrl-C (as `^C`,
/// or the bare control character some terminals show), nor any other key.
/// An echo lands after the live view's line, and a line that fills the
/// width wraps it onto a row the view does not know about, which then keeps
/// a stale copy of the line when the view is cleared.
struct NoEcho {
    #[cfg(unix)]
    saved: Option<nix::sys::termios::Termios>,
}

impl NoEcho {
    fn new() -> Self {
        #[cfg(unix)]
        {
            use nix::sys::termios::{LocalFlags, SetArg, tcgetattr, tcsetattr};
            let echoes = LocalFlags::ECHO | LocalFlags::ECHOCTL;
            let saved = tcgetattr(stderr())
                .ok()
                .filter(|termios| termios.local_flags.intersects(echoes));
            let quieted = saved.as_ref().is_some_and(|saved| {
                let mut quiet = saved.clone();
                quiet.local_flags.remove(echoes);
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

impl Drop for NoEcho {
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

/// Who pays and the quote, a line each and none wider than 80 columns:
///
/// ```text
/// Identity: default
/// Buyer:    7k7px-dcypq-wulnh-byzgd-6sib2-udn6c-lfo6e-6oujd-veifp-62woo-5ae
/// Gateway:  2bwus-iiaaa-aaaac-bee3a-cai (CyclePay)
/// Price:    10.00 USD (9.41 USD after a 0.59 USD card fee)
/// Estimate: about 6.959T cycles, at 1.3520 USD per 1T cycles
/// ```
///
/// The gateway is shown so that the buyer confirms where the order goes
/// (#813's decision), and called CyclePay's only when it is the default.
fn quote_block(
    quote: &Quote,
    target: Target,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(name) = identity {
        lines.push(format!("Identity: {name}"));
    }
    lines.push(format!("Buyer:    {buyer}"));
    let whose = if gateway.canister.to_text() == CYCLES_GATEWAY_CID {
        " (CyclePay)"
    } else {
        ""
    };
    lines.push(format!("Gateway:  {}{whose}", gateway.canister));
    if let Target::Receive { cycles, .. } = target {
        lines.push(format!("Wanted:   at least {}", format_cycles(cycles)));
    }
    lines.push(format!(
        "Price:    {} ({} after a {} card fee)",
        quote.amount, quote.net, quote.fee
    ));
    lines.push(format!(
        "Estimate: about {}, at {} per 1T cycles",
        format_cycles(quote.cycles),
        quote.rate.usd_per_trillion()
    ));
    lines
}

/// Print the Payment Link, and open it where that makes sense: for a person
/// at a terminal who did not ask otherwise, and not over SSH or on CI, where
/// the browser would not be theirs. Agents, containers and scripts (`--json`,
/// `--quiet`) have no browser; they get the link to hand on.
fn announce_payable(order: &OrderView, quote: &Quote, output: Output, no_open: bool) {
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
                fee: quote.fee,
                estimated_cycles: quote.cycles.to_string(),
                expires_at: order.expires_at.map(format_secs),
            }) {
                warn!("failed to write JSON: {e}");
            }
        }
        Output::Quiet => println!("{url}"),
        Output::Human => {
            // The link goes to stdout. Only when both are on the terminal does
            // it follow this line where a colon can introduce it.
            let leads = if stdout().is_terminal() && stderr().is_terminal() {
                ":"
            } else {
                "."
            };
            info!(
                "Order {} is payable{}{leads}",
                order.id,
                payable_for(order, now_secs())
            );
            println!("{url}");
            if !no_open && stderr().is_terminal() && browser_is_at_hand() {
                match open::that(url) {
                    Ok(()) => info!("Opened the Payment Link in your browser."),
                    Err(e) => warn!("Could not open a browser ({e}); open the URL above yourself."),
                }
            }
            // A phone camera is the usual way to reach a card from a terminal;
            // the code is for eyes, so it only goes where there are some.
            let color = std::env::var_os("NO_COLOR").is_none();
            if stderr().is_terminal()
                && let Some(qr) = render_qr(url, color)
            {
                // A code wrapped across lines, or partly scrolled off the
                // screen by the wait's line below it, cannot be scanned.
                let (rows, columns) = Term::stderr().size();
                let width = qr
                    .lines()
                    .next()
                    .map_or(0, |row| row.chars().filter(|c| "█▀▄ ".contains(*c)).count());
                let height = qr.lines().count() + 1;
                if width > usize::from(columns) || height > usize::from(rows) {
                    info!(
                        "(Make the terminal {width} columns by {height} rows or larger to see the \
                         QR code.)"
                    );
                } else {
                    eprintln!("{qr}");
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
            let _quiet = NoEcho::new();
            rendered(debug, wait).await
        }
        Output::Human | Output::Json => {
            info!("If this command is stopped, the order carries on without it.");
            with_heartbeat(HEARTBEAT, |line| info!("{line}"), wait).await
        }
        Output::Quiet => wait(&Reporter::null()).await,
    };

    let order = match result {
        Ok(order) => order,
        // Stopped, not failed: the order carries on without the command.
        Err(CyclesPurchaseError::Interrupted { order }) => {
            if output != Output::Quiet {
                info!("{} {AFTER_HINT}", standing(&order, false, now_secs()));
            }
            return Err(Stopped::INTERRUPTED.into());
        }
        Err(CyclesPurchaseError::WaitTimedOut { order }) => {
            if output != Output::Quiet {
                info!("{} {AFTER_HINT}", standing(&order, true, now_secs()));
            }
            return Err(CyclesPurchaseError::WaitTimedOut { order }.into());
        }
        Err(error) => return Err(error.into()),
    };

    report_end(agent, buyer, &order, output).await
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
                    Some(EventKind::TaskStarted { task, .. }) => {
                        // Not the live view's line, whose Ctrl-C hint is
                        // for a person at the terminal.
                        let begun = match task.phase {
                            PurchasePhase::AwaitingPayment => "Waiting for payment...",
                            PurchasePhase::Delivering => "Payment received; delivering cycles...",
                        };
                        say(format!("[order {}] {begun}", task.order_id));
                        phase = Some(task);
                    }
                    // A stop is said as the live view says it; any other end
                    // is told by the command once the wait returns.
                    Some(EventKind::TaskCompleted { outcome }) => {
                        if let (Some(task), TaskOutcome::Stopped { message }) = (phase.take(), outcome)
                        {
                            say(format!("[order {}] {message}", task.order_id));
                        }
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

/// Tell how a settled order ended, and fail unless it was delivered.
async fn report_end(
    agent: &ic_agent::Agent,
    buyer: Principal,
    order: &OrderView,
    output: Output,
) -> Result<(), anyhow::Error> {
    // The buyer's balance says something only when the cycles went there:
    // an order made on CyclePay's site can top up a canister, or credit
    // another principal.
    let to_buyer = order.destination == Some(Destination::Ledger(buyer));
    let balance = if order.status == OrderStatus::Delivered && to_buyer {
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
        println!("CyclePay reports {cycles} delivered{}", delivered_to(order));
        if let Some(balance) = balance {
            println!("Balance: {}", format_cycles(balance));
        }
    }
    ending(order)
}

/// ` to <principal>`, ` to canister <id>`: where the order says its cycles
/// went, or nothing if it says in a way this CLI does not know.
fn delivered_to(order: &OrderView) -> String {
    match order.destination {
        Some(Destination::Ledger(owner)) => format!(" to {owner}"),
        Some(Destination::Canister(canister)) => format!(" to canister {canister}"),
        None => String::new(),
    }
}

/// How a settled order ended, as the command's result: `Ok` for a delivery,
/// and an error that says what happened otherwise.
fn ending(order: &OrderView) -> Result<(), anyhow::Error> {
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
             order by hand (a disputed payment is settled by the dispute). Keep the order id; \
             CyclePay's site shows the outcome under My purchases",
            order.id
        ),
        OrderStatus::AwaitingPayment
        | OrderStatus::PaymentPending
        | OrderStatus::Processing
        | OrderStatus::Unknown => bail!("order {} is still {}", order.id, order.status),
    }
}

/// Where an order the wait left unsettled stands at `now` (seconds since
/// the Unix epoch), when it was stopped or (`timed_out`) gave up. Only an
/// order known to be unpaid is payable.
fn standing(order: &OrderView, timed_out: bool, now: u64) -> String {
    match order.status {
        OrderStatus::AwaitingPayment if timed_out => format!(
            "No payment arrived. Order {} stays payable{}.",
            order.id,
            payable_for(order, now)
        ),
        OrderStatus::AwaitingPayment => format!(
            "Order {} stays payable{}.",
            order.id,
            payable_for(order, now)
        ),
        status if timed_out => format!("Order {} is still {status}.", order.id),
        status => format!("Order {} is {status}.", order.id),
    }
}

/// ` for 25 hours (until 2026-10-14 03:33 UTC)`: how much longer an unpaid
/// order can be paid, from `now` (seconds since the Unix epoch), and until
/// when, in UTC to the minute. Nothing when the order has no deadline.
fn payable_for(order: &OrderView, now: u64) -> String {
    let Some(deadline) = order.expires_at else {
        return String::new();
    };
    let until = format_utc(deadline);
    match deadline.checked_sub(now).filter(|left| *left > 0) {
        Some(left) => format!(" for {} (until {until})", format_left(left)),
        None => format!(" until {until}"),
    }
}

/// `25 hours`, `40 minutes`: a time left, to the nearest hour from an hour
/// and a half, and to the nearest minute below.
fn format_left(secs: u64) -> String {
    let (count, unit) = if secs >= 90 * 60 {
        ((secs + 1_800) / 3_600, "hour")
    } else {
        (((secs + 30) / 60).max(1), "minute")
    };
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

/// Seconds since the Unix epoch as `2026-10-14 03:33 UTC`, for prose.
fn format_utc(timestamp: u64) -> String {
    OffsetDateTime::from_unix_timestamp(timestamp as i64)
        .map(|dt| {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02} UTC",
                dt.year(),
                u8::from(dt.month()),
                dt.day(),
                dt.hour(),
                dt.minute()
            )
        })
        .unwrap_or_else(|_| timestamp.to_string())
}

/// The URL as a QR code drawn with half-block characters, two modules per
/// row, for scanning off the terminal. It takes the lowest error correction
/// (a screen does not smudge) and a two-module quiet zone, the least a
/// scanner needs, so that the Payment Link's code fits an 80x24 terminal with
/// the wait's line below it: 41x21, or 45x23 once the order id and amount
/// make the URL longer than 106 bytes. `None` only if the URL is too long to
/// encode.
///
/// With `color`, every cell is an upper half block whose foreground is the
/// top module's colour and whose background is the bottom module's: bright
/// white for light, black for dark. The code reads the same on a light theme
/// as on a dark one, and any spacing a terminal adds between its rows takes
/// the colour of the module just above it, rather than cutting a dark line
/// through light ones. Without, light modules are the blocks, in the
/// terminal's own colours, which scans best light on dark.
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
    let mut rows = (0..size).step_by(2).map(|y| {
        if !color {
            return (0..size)
                .map(|x| match (light(x, y), light(x, y + 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                })
                .collect();
        }
        // The colours are set where they change, and reset at the row's end.
        let mut row = String::new();
        let mut set = None;
        for x in 0..size {
            let cell = (light(x, y), light(x, y + 1));
            if set != Some(cell) {
                let fg = if cell.0 { 97 } else { 30 };
                let bg = if cell.1 { 107 } else { 40 };
                row.push_str(&format!("\x1b[{fg};{bg}m"));
                set = Some(cell);
            }
            row.push('▀');
        }
        row.push_str("\x1b[0m");
        row
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
    /// The card fee included in `amount`.
    fee: Money,
    /// About the cycles `net` buys at the rate quoted, as a decimal string.
    estimated_cycles: String,
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

    fn gateway_at(canister: &str) -> Gateway {
        Gateway {
            canister: Principal::from_text(canister).unwrap(),
            paused: false,
            terms: crate::cyclepay::pricing::Terms {
                min_order_usd_cents: 100,
                max_order_usd_cents: 20_000,
                fee_bps: 290,
                fee_fixed_usd_cents: 30,
                ledger_delivery_fee_cycles: 200_000_000,
            },
        }
    }

    #[test]
    fn the_quote_block_shows_amount_cycles_fee_and_rate() {
        let quote = Quote {
            amount: Money::from_minor_units(1000, Currency::Usd),
            fee: Money::from_minor_units(59, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            cycles: 6_959_866_863_905,
            rate: Rate {
                xdr_usd_x10000: 13_520,
                stale: false,
                age: None,
            },
        };
        let buyer = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let block = quote_block(
            &quote,
            Target::Spend(quote.amount),
            &gateway_at(CYCLES_GATEWAY_CID),
            buyer,
            Some("default"),
        );
        assert_eq!(
            block,
            [
                "Identity: default",
                "Buyer:    rrkah-fqaaa-aaaaa-aaaaq-cai",
                "Gateway:  2bwus-iiaaa-aaaac-bee3a-cai (CyclePay)",
                "Price:    10.00 USD (9.41 USD after a 0.59 USD card fee)",
                "Estimate: about 6.959T cycles, at 1.3520 USD per 1T cycles",
            ]
        );
        // Another gateway is not said to be CyclePay's.
        let block = quote_block(
            &quote,
            Target::Spend(quote.amount),
            &gateway_at("ryjl3-tyaaa-aaaaa-aaaba-cai"),
            buyer,
            None,
        );
        assert_eq!(block[1], "Gateway:  ryjl3-tyaaa-aaaaa-aaaba-cai");
    }

    /// The widest the block can be, a self-authenticating principal (the
    /// longest there is) buying the most an order allows, fits 80 columns.
    #[test]
    fn the_quote_block_fits_80_columns() {
        let quote = Quote {
            amount: Money::from_minor_units(20_000, Currency::Usd),
            fee: Money::from_minor_units(610, Currency::Usd),
            net: Money::from_minor_units(19_390, Currency::Usd),
            cycles: 999_999_999_999_999,
            rate: Rate {
                xdr_usd_x10000: 13_520,
                stale: false,
                age: None,
            },
        };
        let buyer =
            Principal::from_text("7k7px-dcypq-wulnh-byzgd-6sib2-udn6c-lfo6e-6oujd-veifp-62woo-5ae")
                .unwrap();
        let block = quote_block(
            &quote,
            Target::Receive {
                cycles: 999_999_999_999_999,
                currency: Currency::Usd,
            },
            &gateway_at(CYCLES_GATEWAY_CID),
            buyer,
            Some("a-rather-long-identity-name"),
        );
        for line in &block {
            assert!(
                line.chars().count() <= 80,
                "{} columns: {line}",
                line.chars().count()
            );
        }
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
        // In colour, every cell is an upper half block, and every row sets
        // its colours first and resets them after.
        let colored = render_qr(url, true).unwrap();
        assert!(
            colored
                .lines()
                .all(|l| l.starts_with("\x1b[") && l.ends_with("\x1b[0m")),
            "{colored}"
        );
        assert!(
            colored
                .lines()
                .all(|l| l.chars().filter(|c| "█▀▄ ".contains(*c)).count() == width),
            "{colored}"
        );
        assert!(!colored.contains(['█', '▄', ' ']), "{colored}");
        // Far beyond a QR code's capacity.
        assert!(render_qr(&"x".repeat(10_000), false).is_none());
    }

    /// Each cell of a drawn code as (top light, bottom light): read off the
    /// block characters without colour, and off each cell's foreground
    /// (top) and background (bottom) with it.
    fn read_back(qr: &str, color: bool) -> Vec<Vec<(bool, bool)>> {
        qr.lines()
            .map(|line| {
                let mut cells = Vec::new();
                let (mut fg, mut bg) = (None, None);
                let mut chars = line.chars();
                while let Some(c) = chars.next() {
                    if c == '\x1b' {
                        let sgr: String = chars.by_ref().take_while(|c| *c != 'm').collect();
                        let codes: Vec<&str> = sgr.trim_start_matches('[').split(';').collect();
                        match codes[..] {
                            ["0"] => (fg, bg) = (None, None),
                            [f, b] => (fg, bg) = (Some(f.to_owned()), Some(b.to_owned())),
                            _ => panic!("unexpected SGR {sgr:?}"),
                        }
                        continue;
                    }
                    cells.push(if color {
                        assert_eq!(c, '▀', "{line:?}");
                        let light = |code: &Option<String>, on: &str, off: &str| match code {
                            Some(code) if code == on => true,
                            Some(code) if code == off => false,
                            other => panic!("unexpected colour {other:?} in {line:?}"),
                        };
                        (light(&fg, "97", "30"), light(&bg, "107", "40"))
                    } else {
                        match c {
                            '█' => (true, true),
                            '▀' => (true, false),
                            '▄' => (false, true),
                            ' ' => (false, false),
                            c => panic!("unexpected {c:?}"),
                        }
                    });
                }
                cells
            })
            .collect()
    }

    /// Read back, with colour and without, the drawing is the code's own
    /// modules inside a light quiet zone two modules wide, with the half row
    /// below its odd height light too: a dark stripe there would leave a
    /// scanner no edge to find.
    #[test]
    fn qr_draws_every_module_where_it_is() {
        use qrcode::{Color, EcLevel, QrCode};
        let url = "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00?client_reference_id=o_co-4-77fd4ac0&prefilled_amount=100";
        let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L).unwrap();
        let (width, modules) = (code.width(), code.to_colors());
        let size = width + 4;
        for color in [false, true] {
            let rows = read_back(&render_qr(url, color).unwrap(), color);
            assert_eq!(rows.len(), size.div_ceil(2));
            assert!(rows.iter().all(|row| row.len() == size));
            for y in 0..rows.len() * 2 {
                for x in 0..size {
                    let (top, bottom) = rows[y / 2][x];
                    let drawn_light = if y % 2 == 0 { top } else { bottom };
                    let inside = (2..2 + width).contains(&x) && (2..2 + width).contains(&y);
                    let light = !inside || modules[(y - 2) * width + (x - 2)] == Color::Light;
                    assert_eq!(drawn_light, light, "module ({x}, {y}), color {color}");
                }
            }
        }
    }

    /// Only a delivery succeeds; every other end is an error that says what
    /// happened.
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
            destination: None,
        };
        assert!(ending(&order(OrderStatus::Delivered)).is_ok());
        for (status, says) in [
            (OrderStatus::Refunded, "10.00 USD was refunded"),
            (OrderStatus::Refunding, "is being refunded"),
            (OrderStatus::RefundOwed, "owes you a refund"),
            (OrderStatus::NeedsReview, "Keep the order id"),
            (OrderStatus::Expired, "expired unpaid"),
        ] {
            let err = ending(&order(status)).unwrap_err().to_string();
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
            destination: None,
        };
        let now = 1_700_000_000 - 25 * 3_600 + 10;
        assert_eq!(
            standing(&order(OrderStatus::AwaitingPayment), false, now),
            "Order co-1 stays payable for 25 hours (until 2023-11-14 22:13 UTC)."
        );
        assert_eq!(
            standing(&order(OrderStatus::AwaitingPayment), true, now),
            "No payment arrived. Order co-1 stays payable for 25 hours (until 2023-11-14 22:13 \
             UTC)."
        );
        // Within the grace a wait allows past the deadline.
        assert_eq!(
            standing(&order(OrderStatus::AwaitingPayment), false, 1_700_000_030),
            "Order co-1 stays payable until 2023-11-14 22:13 UTC."
        );
        assert_eq!(
            standing(&order(OrderStatus::Processing), true, now),
            "Order co-1 is still paid, delivering."
        );
        for timed_out in [false, true] {
            let said = standing(&order(OrderStatus::Unknown), timed_out, now);
            assert!(said.contains("does not know"), "{said}");
            assert!(
                !said.contains("payable") && !said.contains("No payment"),
                "{said}"
            );
        }
    }

    /// Off a terminal, a wait says when each phase begins and, every
    /// minute, that it goes on, until it ends. On a paused clock: 150 s
    /// awaiting payment, then 130 s delivering, so no beat falls on a change.
    #[tokio::test(start_paused = true)]
    async fn a_wait_with_nothing_drawn_keeps_saying_it_goes_on() {
        let said = std::sync::Mutex::new(Vec::new());
        let ended = with_heartbeat(
            HEARTBEAT,
            |line| said.lock().unwrap().push(line),
            async |reporter: &Reporter| {
                for (phase, secs) in [
                    (PurchasePhase::AwaitingPayment, 150),
                    (PurchasePhase::Delivering, 130),
                ] {
                    let task = reporter.task(CyclesPurchaseTask::new("co-1", phase));
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    task.finish(TaskOutcome::succeeded());
                }
                "delivered"
            },
        )
        .await;
        assert_eq!(ended, "delivered");
        assert_eq!(
            said.into_inner().unwrap(),
            [
                "[order co-1] Waiting for payment...",
                "[order co-1] Still waiting for payment (1 minute so far)",
                "[order co-1] Still waiting for payment (2 minutes so far)",
                "[order co-1] Payment received; delivering cycles...",
                "[order co-1] Payment received; still delivering cycles (3 minutes so far)",
                "[order co-1] Payment received; still delivering cycles (4 minutes so far)",
            ]
        );
    }

    /// A delivery is told against the place the order says, whichever it is.
    #[test]
    fn a_delivery_names_where_the_cycles_went() {
        let principal = |text| Principal::from_text(text).unwrap();
        let order = |destination| OrderView {
            id: "co-1".into(),
            status: OrderStatus::Delivered,
            amount: Money::from_minor_units(1000, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            checkout_url: None,
            expires_at: None,
            paid: Some(Money::from_minor_units(1000, Currency::Usd)),
            cycles_delivered: Some(6_959_866_863_905),
            refunded: None,
            detail: None,
            destination,
        };
        let buyer = principal("rrkah-fqaaa-aaaaa-aaaaq-cai");
        let canister = principal("ryjl3-tyaaa-aaaaa-aaaba-cai");
        assert_eq!(
            delivered_to(&order(Some(Destination::Ledger(buyer)))),
            " to rrkah-fqaaa-aaaaa-aaaaq-cai"
        );
        assert_eq!(
            delivered_to(&order(Some(Destination::Canister(canister)))),
            " to canister ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
        assert_eq!(delivered_to(&order(None)), "");
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

    /// Off a terminal, a stopped wait says so, as the live view does.
    #[tokio::test(start_paused = true)]
    async fn a_wait_with_nothing_drawn_says_it_was_stopped() {
        let said = std::sync::Mutex::new(Vec::new());
        with_heartbeat(
            HEARTBEAT,
            |line| said.lock().unwrap().push(line),
            async |reporter: &Reporter| {
                let task = reporter.task(CyclesPurchaseTask::new(
                    "co-1",
                    PurchasePhase::AwaitingPayment,
                ));
                tokio::time::sleep(Duration::from_secs(70)).await;
                task.finish(TaskOutcome::Stopped {
                    message: "stopped waiting".to_owned(),
                });
            },
        )
        .await;
        assert_eq!(
            said.into_inner().unwrap(),
            [
                "[order co-1] Waiting for payment...",
                "[order co-1] Still waiting for payment (1 minute so far)",
                "[order co-1] stopped waiting",
            ]
        );
    }

    /// A Ctrl-C caught at the prompt is kept until asked for, once; without
    /// one, asking does not wait. SIGINT is raised at this test process,
    /// whose handler the prompt's listener has just installed.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_ctrl_c_caught_at_the_prompt_is_kept_for_after_the_order() {
        use nix::sys::signal::{Signal, raise};

        assert!(!Interrupt::later().pressed());
        let mut interrupt = Interrupt::catch_at_prompt().unwrap();
        assert!(!interrupt.pressed());
        raise(Signal::SIGINT).unwrap();
        let mut pressed = false;
        for _ in 0..100 {
            if interrupt.pressed() {
                pressed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(pressed, "the Ctrl-C was not seen");
        assert!(!interrupt.pressed(), "one Ctrl-C was seen twice");
    }

    #[test]
    fn timestamps_and_ages_read_well() {
        assert_eq!(format_secs(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14 22:13 UTC");
        assert_eq!(format_left(25 * 3_600 - 15), "25 hours");
        assert_eq!(format_left(90 * 60), "2 hours");
        assert_eq!(format_left(89 * 60), "89 minutes");
        assert_eq!(format_left(61), "1 minute");
        assert_eq!(format_left(5), "1 minute");
        assert_eq!(format_age(Duration::from_secs(90)), "1 minute");
        assert_eq!(format_age(Duration::from_secs(5 * 3600)), "5 hours");
        assert_eq!(format_age(Duration::from_secs(3 * 86_400)), "3 days");
    }
}
