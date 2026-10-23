use std::io::{IsTerminal, stderr, stdin, stdout};
use std::time::Duration;

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::{ArgGroup, Args};
use dialoguer::Confirm;
use dialoguer::console::{Term, colors_enabled_stderr};
use icp_app::context::{Context, GetAgentError, GetAgentForEnvError, GetAgentForNetworkError};
use icp_app::identity::IdentitySelection;
use icp_app::identity::manifest::IdentityDefaults;
use icp_app::operations::token::balance::get_raw_balance;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icp_project::calls::CanisterCalls;
use icp_project::parsers::CyclesAmount;
use icp_project::signal::stop_signal;
use num_traits::ToPrimitive;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};

use crate::commands::args::{
    PasteFlags, TokenCommandArgs, TokenCommandSelections, selection_flags, shell_word,
};
use crate::cyclepay::format_cycles;
use crate::cyclepay::interface::{
    CardOrderStatus, Destination, ORDER_STATUS_METHOD, cycles_gateway_canister_id, is_cyclepay,
};
use crate::cyclepay::money::{Currency, Money};
use crate::cyclepay::purchase::{
    self as purchase, CyclesPurchaseError, Gateway, MAINNET_HINT, OrderView, PurchasePhase, Quote,
    Rate, Target, WaitOptions,
};
use crate::render::{ProgressManager, ProgressManagerSettings};

/// Buy cycles with a card.
///
/// Creates an order on a cycles gateway canister as the current identity,
/// prints the page to pay on (and opens it in a browser when run from a
/// terminal, but not over SSH or on CI), then waits for the cycles to land on
/// the identity's own cycles-ledger account. Fund a canister from there with
/// `icp canister top-up`. Say either how much to spend (--amount, in the
/// currency given by --currency) or how many cycles to receive (--cycles); the
/// latter pays the least amount that buys them once the card fee is taken
/// out. The gateway publishes its per-order range and its fees, and the quote
/// shows them. The default gateway is CyclePay's, on mainnet, which accepts
/// USD only.
///
/// Exactly one of --amount or --cycles must be given.
#[derive(Debug, Args)]
#[command(group(ArgGroup::new("what").required(true).args(["amount", "cycles"])))]
pub(crate) struct BuyArgs {
    /// Amount to spend, as a decimal (e.g. 10 or 12.50), in the currency given by --currency
    #[arg(
        long,
        value_name = "AMOUNT",
        allow_hyphen_values = true,
        value_parser = parse_amount
    )]
    pub(crate) amount: Option<String>,

    /// Cycles to receive; the amount to pay is worked out from the current rate, card fee included.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(
        long,
        value_name = "CYCLES",
        allow_hyphen_values = true,
        value_parser = parse_cycles
    )]
    pub(crate) cycles: Option<CyclesAmount>,

    /// ISO 4217 code of the currency to pay in. CyclePay accepts USD only
    #[arg(long, value_name = "CODE", default_value = "USD")]
    pub(crate) currency: Currency,

    /// Canister id of the cycles gateway to buy through: any canister serving the cycles gateway
    /// interface. Defaults to CyclePay's backend on mainnet, or to ICP_CYCLES_GATEWAY_CANISTER_ID
    /// when set
    #[arg(long, value_name = "CANISTER_ID")]
    pub(crate) gateway: Option<Principal>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// Skip confirmation prompts. Implied by --json and --quiet, which have nobody to ask
    #[arg(long, short)]
    pub(crate) yes: bool,

    /// Do not open the payment page in a browser
    #[arg(long)]
    pub(crate) no_open: bool,

    /// Print the payment page's link and return without waiting for the order to complete
    #[arg(long)]
    pub(crate) no_wait: bool,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the payment page's link, then the final status
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

/// `--amount` as given, but for a sign, which is refused here as `--cycles`
/// refuses one; the rest is read once the currency is known.
fn parse_amount(input: &str) -> Result<String, String> {
    unsigned(input, "the amount to spend, such as 10 or 12.50")?;
    Ok(input.to_owned())
}

/// `--cycles`, refusing a sign as `--amount` does.
fn parse_cycles(input: &str) -> Result<CyclesAmount, String> {
    unsigned(input, "the cycles to receive, such as 5t")?;
    input.parse()
}

/// Refuse a value that starts with `-`, which both options take as theirs
/// rather than as a flag: a negative number (`-1`, `-5t`), or a flag given
/// where the value was left out (`--amount --yes`). Said in words, where clap
/// would read `-5t` as an unknown flag.
fn unsigned(input: &str, wanted: &str) -> Result<(), String> {
    match input.trim_start().strip_prefix('-') {
        None => Ok(()),
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit() || c == '.') => {
            Err(format!("cannot be negative: give {wanted}"))
        }
        Some(_) => Err(format!("give {wanted}")),
    }
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
    // A principal that cannot be a canister is said so before any network
    // is asked about it.
    purchase::check_gateway_id(canister)?;
    let cyclepay = is_cyclepay(canister);

    let agent = match ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await
    {
        Ok(agent) => agent,
        // No network to reach: none named outside a project, or the one
        // chosen is a local one that is not running. CyclePay's gateway is
        // on mainnet only, which is always reachable.
        Err(
            error @ (GetAgentError::NoProjectOrNetwork
            | GetAgentError::GetAgentForEnv {
                source: GetAgentForEnvError::NetworkAccess { .. },
            }
            | GetAgentError::GetAgentForNetwork {
                source: GetAgentForNetworkError::NetworkAccess { .. },
            }),
        ) if cyclepay => {
            bail!("{error}. {MAINNET_HINT}")
        }
        Err(error) => return Err(error.into()),
    };
    // CyclePay's gateway is on mainnet only: refuse any other network before
    // asking it anything.
    purchase::check_network(canister, &agent.read_root_key())?;
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
    let (order, quote) =
        create_order(calls, &gateway, buyer, identity.as_deref(), target, args).await?;
    let after = FollowUp {
        flags: paste_flags(&selections, identity.as_deref()),
        canister,
    };
    announce_payable(&order, &quote, output, args.no_open);

    if args.no_wait {
        if output == Output::Human {
            let mut paragraphs = vec![
                "Not waiting. Once the order is paid, its cycles land on your identity's \
                 cycles-ledger account. To see them:"
                    .to_owned(),
            ];
            after.balance(&mut paragraphs);
            info!("{}", paragraphs.join("\n\n"));
        }
        return Ok(());
    }

    wait_and_report(
        calls, &agent, &gateway, buyer, order, output, ctx.debug, &after,
    )
    .await
}

/// What the gateway is called in prose. CyclePay's own is named, with
/// Stripe, which takes its card payments; nothing of either is said of any
/// other gateway, which may sell some other way.
#[derive(Clone, Copy)]
pub(crate) struct Names {
    /// At the start of a sentence: `CyclePay`, `The gateway`.
    pub(crate) subject: &'static str,
    /// Within one: `CyclePay`, `the gateway`.
    pub(crate) name: &'static str,
    /// The page a card is paid on.
    pub(crate) page: &'static str,
    pub(crate) cyclepay: bool,
}

impl Names {
    pub(crate) fn of(gateway: &Gateway) -> Self {
        if is_cyclepay(gateway.canister) {
            Self {
                subject: "CyclePay",
                name: "CyclePay",
                page: "Stripe's page",
                cyclepay: true,
            }
        } else {
            Self {
                subject: "The gateway",
                name: "the gateway",
                page: "the gateway's payment page",
                cyclepay: false,
            }
        }
    }
}

/// The flags that select what this run selected, for a command printed to
/// be pasted. The identity is named even when it was the default: only the
/// identity that made an order can read it, and the default can change.
fn paste_flags(selections: &TokenCommandSelections, identity: Option<&str>) -> PasteFlags {
    let identity = match (&selections.identity, identity) {
        (IdentitySelection::Default, Some(name)) => IdentitySelection::Named(name.to_owned()),
        (selected, _) => selected.clone(),
    };
    selection_flags(&identity, &selections.network, &selections.environment)
}

/// `commands` as a paragraph of their own, each on a line of its own,
/// indented and never wrapped; then a paragraph for any blank in `flags`
/// that the reader fills in.
fn commands_with(paragraphs: &mut Vec<String>, commands: &[String], flags: &PasteFlags) {
    paragraphs.push(
        commands
            .iter()
            .map(|command| format!("    {command}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    paragraphs.extend(flags.notes());
}

/// How to look in on an order once the command has stopped waiting on it,
/// as the identity and on the network the command ran with.
struct FollowUp {
    flags: PasteFlags,
    /// The gateway the order was made on.
    canister: Principal,
}

impl FollowUp {
    /// The command that shows the cycles once they land.
    fn balance(&self, paragraphs: &mut Vec<String>) {
        let command = format!("icp cycles balance{}", self.flags.text);
        commands_with(paragraphs, &[command], &self.flags);
    }

    /// The command that reads order `id` back.
    fn order_status(&self, paragraphs: &mut Vec<String>, id: &str) {
        let arg = shell_word(&format!("(\"{}\")", id.escape_default()))
            .unwrap_or_else(|| "'(\"<order id>\")'".to_owned());
        let command = format!(
            "icp canister call {} {ORDER_STATUS_METHOD} --query{} {arg}",
            self.canister, self.flags.text
        );
        commands_with(paragraphs, &[command], &self.flags);
    }
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
    let (yes, speak) = consent(args);
    confirm(&quote, target, gateway, buyer, identity, yes, speak)?;
    match purchase::place_order(calls, gateway, quote.amount).await {
        Ok(order) => Ok((order, quote)),
        Err(CyclesPurchaseError::OrderRefused { error }) => {
            bail!("{} refused the order: {error}", Names::of(gateway).subject)
        }
        Err(error) => Err(error.into()),
    }
}

/// Whether the order is already consented to (`yes`), and whether to show the
/// quote and say so (`speak`). A machine-readable run, `--json` or `-q`, has
/// nobody to ask and reads the figures off stdout: it implies `--yes`, and the
/// quote stays off stderr. A person who said `--yes` outright is still shown
/// the quote.
fn consent(args: &BuyArgs) -> (bool, bool) {
    let machine = Output::of(args) != Output::Human;
    (args.yes || machine, !machine)
}

/// Show what is about to happen and ask. Without a terminal there is nobody
/// to ask, so `--yes` is required; a run that does not `speak` has implied it.
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
    let names = Names::of(gateway);
    if speak {
        for line in quote_block(quote, target, gateway, buyer, identity) {
            info!("{line}");
        }
    }
    // Said even when the quote is not: a script that reads the figures off
    // --json, or -q's, would otherwise never hear they may be off.
    if let Some(warning) = stale_rate_warning(&quote.rate, names) {
        warn!("{warning}");
    }
    if !speak {
        return Ok(());
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
        .with_prompt(format!(
            "Create this order? Nothing is charged until you pay on {}.",
            names.page
        ))
        .default(false)
        .interact()?;
    if !confirmed {
        bail!("no order created");
    }
    Ok(())
}

/// What to say of a rate the gateway has not refreshed for longer than it
/// tolerates, if it has not.
fn stale_rate_warning(rate: &Rate, names: Names) -> Option<String> {
    let age = rate
        .age
        .map(|age| format!(" for {}", format_age(age)))
        .unwrap_or_default();
    rate.stale.then(|| {
        format!(
            "{} has not refreshed its exchange rate{age}; the quote may be off, and the \
             cycles delivered follow the rate when the payment lands.",
            names.subject
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
    let whose = if is_cyclepay(gateway.canister) {
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
            order.id,
            order.status_text()
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
                payable_until(order)
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
            if stderr().is_terminal()
                && let Some(qr) = render_qr(url)
            {
                // A code wrapped across lines, or partly scrolled off the
                // screen by the wait's line below it, cannot be scanned.
                let (rows, columns) = Term::stderr().size();
                let width = qr.lines().next().map_or(0, |row| row.chars().count());
                let height = qr.lines().count() + 1;
                if width > usize::from(columns) || height > usize::from(rows) {
                    info!(
                        "(Make the terminal {width} columns by {height} rows or larger to see the \
                         QR code.)"
                    );
                } else if colors_enabled_stderr() {
                    eprintln!("{}", black_on_white(&qr));
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
    output: Output,
    debug: bool,
    after: &FollowUp,
) -> Result<(), anyhow::Error> {
    let names = Names::of(gateway);
    let id = order.id.clone();
    // The live line is for a person at a terminal. Anyone else reading
    // stderr as it comes (an agent, a log, the debug log a spinner would
    // interleave with) is told in lines instead; -q asks for nothing but the
    // link and the end.
    let live = output == Output::Human && stderr().is_terminal() && !debug;
    let progress = live.then(|| ProgressManager::new(ProgressManagerSettings { hidden: false }));
    let bar = progress.as_ref().map(|progress| {
        let bar = progress.create_independent_progress_bar();
        bar.set_prefix(format!("[order {}]", order_number(&id)));
        bar
    });
    if !live && output != Output::Quiet {
        info!("If this command is stopped, the order carries on without it.");
    }
    let on_phase = |phase: PurchasePhase| match &bar {
        Some(bar) => bar.set_message(live_message(phase)),
        None if output != Output::Quiet => info!("[order {id}] {}", line_message(phase)),
        None => {}
    };
    let options = WaitOptions::default();
    let wait = purchase::wait_for_order(
        calls,
        gateway,
        &id,
        Some(order),
        &options,
        stop_signal(),
        on_phase,
    );
    let result = match &bar {
        None => wait.await,
        // Only a delivery ends the line well; any other end, the wait's or
        // the order's, is said on it in a few words. The command then tells
        // the whole of it.
        Some(bar) => ProgressManager::execute_with_progress(
            bar,
            async {
                match wait.await {
                    Ok(order) if order.status == Some(CardOrderStatus::Delivered) => Ok(order),
                    Ok(order) => Err(Ended::Order(order)),
                    Err(error) => Err(Ended::Error(error)),
                }
            },
            || "Cycles delivered".to_owned(),
            |ended| match ended {
                Ended::Order(order) => format!("order {}", order.status_text()),
                Ended::Error(CyclesPurchaseError::Interrupted { .. }) => {
                    "stopped waiting".to_owned()
                }
                Ended::Error(CyclesPurchaseError::WaitTimedOut { .. }) => {
                    "gave up waiting".to_owned()
                }
                Ended::Error(CyclesPurchaseError::OrderNotFound { .. }) => {
                    "order not found".to_owned()
                }
                Ended::Error(_) => "lost contact with the gateway".to_owned(),
            },
        )
        .await
        .or_else(|ended| match ended {
            Ended::Order(order) => Ok(order),
            Ended::Error(error) => Err(error),
        }),
    };

    let order = match result {
        Ok(order) => order,
        // -q asks for nothing but the link and the end: the error alone.
        Err(error) if output == Output::Quiet => return Err(error.into()),
        // The wait ended with the order unsettled. It carries on without
        // the command, which says once where it stands, as the error it
        // ends with.
        Err(CyclesPurchaseError::Interrupted { order }) => {
            bail!("{}", unsettled(&order, Unsettled::Stopped, names, after))
        }
        Err(CyclesPurchaseError::WaitTimedOut { order }) => {
            bail!("{}", unsettled(&order, Unsettled::TimedOut, names, after))
        }
        // The gateway stopped answering for five minutes: the order is said
        // as last seen, which is all that is known of it, over the last
        // poll's failure.
        Err(CyclesPurchaseError::ContactLost { order, source, .. }) => {
            return Err(anyhow::Error::new(*source).context(unsettled(
                &order,
                Unsettled::ContactLost,
                names,
                after,
            )));
        }
        Err(error) => return Err(error.into()),
    };

    report_end(agent, buyer, &order, output, names, after).await
}

/// How a wait ended other than in a delivery.
enum Ended {
    /// Settled, but not delivered.
    Order(OrderView),
    Error(CyclesPurchaseError),
}

/// The live line's message for a phase. With its `[order co-<n>] ✷ ` prefix
/// this is 66 columns plus the digits of the order's number, so within 80 up
/// to 14 of them.
fn live_message(phase: PurchasePhase) -> &'static str {
    match phase {
        PurchasePhase::AwaitingPayment => "Waiting for payment; Ctrl-C leaves the order payable",
        PurchasePhase::Delivering => "Payment received; delivering cycles...",
    }
}

/// What a line off a terminal says of a phase: not the live line's, whose
/// Ctrl-C hint is for a person at the terminal.
fn line_message(phase: PurchasePhase) -> &'static str {
    match phase {
        PurchasePhase::AwaitingPayment => "Waiting for payment...",
        PurchasePhase::Delivering => "Payment received; delivering cycles...",
    }
}

/// `co-1234` for CyclePay's `co-1234-<32 hex>`: the order's number, which
/// no other order shares, without the random part, which only paying the
/// order needs. It keeps the live line within 80 columns whatever the id's
/// length; the whole id is printed with the link and in the lines off a
/// terminal. An id of any other shape is shown whole.
fn order_number(order_id: &str) -> &str {
    match order_id.match_indices('-').nth(1) {
        Some((at, _)) => &order_id[..at],
        None => order_id,
    }
}

/// Tell how a settled order ended, and fail unless it was delivered.
async fn report_end(
    agent: &ic_agent::Agent,
    buyer: Principal,
    order: &OrderView,
    output: Output,
    names: Names,
    after: &FollowUp,
) -> Result<(), anyhow::Error> {
    // The buyer's balance says something only when the cycles went there:
    // an order made on CyclePay's site can top up a canister, or credit
    // another principal.
    let to_buyer = order.destination == Some(Destination::Ledger(buyer));
    let balance = if order.status == Some(CardOrderStatus::Delivered) && to_buyer {
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

    if order.status == Some(CardOrderStatus::Delivered) && output == Output::Human {
        let cycles = order
            .cycles_delivered
            .map(format_cycles)
            .unwrap_or_else(|| "cycles".to_owned());
        // The status is read by query, so it is the gateway's report as
        // one replica gives it, and is said to be.
        println!(
            "{} reports {cycles} delivered{}",
            names.subject,
            delivered_to(order)
        );
        if let Some(balance) = balance {
            println!("Balance: {}", format_cycles(balance));
        }
    }
    ending(order, names, after)
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
/// and an error that says what happened otherwise. An order an operator
/// handles comes with the command that shows how it ends.
fn ending(order: &OrderView, names: Names, after: &FollowUp) -> Result<(), anyhow::Error> {
    let detail = order
        .detail
        .as_deref()
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    let id = &order.id;
    let mut paragraphs = Vec::new();
    match order.status {
        Some(CardOrderStatus::Delivered) => return Ok(()),
        Some(CardOrderStatus::Refunding) => {
            bail!("order {id} could not be delivered and is being refunded to your card{detail}")
        }
        Some(CardOrderStatus::Refunded) => bail!(
            "order {id} could not be delivered; {} was refunded to your card{detail}",
            order
                .refunded
                .map(|m| m.to_string())
                .unwrap_or_else(|| "the payment".to_owned())
        ),
        Some(CardOrderStatus::RefundOwed) => {
            paragraphs.push(format!(
                "order {id} could not be delivered: {} owes you a refund, which an operator \
                 makes by hand{detail}.",
                names.name
            ));
            look_later(&mut paragraphs, id, names, after);
        }
        // Not a promise of a refund: a payment disputed before delivery
        // also needs review, and the dispute settles it.
        Some(CardOrderStatus::NeedsReview) => {
            paragraphs.push(format!(
                "order {id} needs review{detail}: your card was charged, and {} checks the order \
                 by hand (a disputed payment is settled by the dispute).",
                names.name
            ));
            look_later(&mut paragraphs, id, names, after);
        }
        Some(
            CardOrderStatus::AwaitingPayment
            | CardOrderStatus::PaymentPending
            | CardOrderStatus::Processing,
        )
        | None => bail!("order {id} is still {}", order.status_text()),
    }
    bail!("{}", paragraphs.join("\n\n"))
}

/// Where to see how an order an operator handles ends: the gateway shows it
/// to the identity that made the order, and CyclePay's site to an identity
/// linked there.
fn look_later(paragraphs: &mut Vec<String>, id: &str, names: Names, after: &FollowUp) {
    paragraphs.push(
        if names.cyclepay {
            "Keep the order id. CyclePay's site shows how it ends under My purchases, and so does:"
        } else {
            "Keep the order id. To see how it ends:"
        }
        .to_owned(),
    );
    after.order_status(paragraphs, id);
}

/// How a wait ended with the order unsettled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unsettled {
    /// Ctrl-C, or a stop signal.
    Stopped,
    /// The hour ran out.
    TimedOut,
    /// The gateway stopped answering for long enough.
    ContactLost,
}

/// Where an order the wait left unsettled stands, said once: why the wait
/// ended, whether the order is still payable or already paid, where its
/// cycles land, and the command that shows them. Only an order known to be
/// unpaid is called payable; one the wait lost contact over may have been
/// paid since, so both are said.
fn unsettled(order: &OrderView, how: Unsettled, names: Names, after: &FollowUp) -> String {
    let id = &order.id;
    let until = payable_until(order);
    let land = "its cycles land on your identity's cycles-ledger account. To see them:";
    let mut paragraphs = vec![match how {
        Unsettled::Stopped => format!("stopped waiting for order {id}."),
        Unsettled::TimedOut | Unsettled::ContactLost => {
            format!("gave up waiting for order {id}.")
        }
    }];
    // Why, unless it was asked for.
    let why = match how {
        Unsettled::Stopped => String::new(),
        Unsettled::TimedOut => "An hour passed. ".to_owned(),
        Unsettled::ContactLost => format!("{} stopped answering. ", names.subject),
    };
    paragraphs.push(match (order.status, how) {
        (Some(CardOrderStatus::AwaitingPayment), Unsettled::ContactLost) => format!(
            "{why}The order carries on without this command. Unpaid, it stays payable{until}; \
             once it is paid, {land}"
        ),
        (Some(CardOrderStatus::AwaitingPayment), Unsettled::TimedOut) => format!(
            "No payment arrived in an hour. It stays payable{until}; once it is paid, {land}"
        ),
        (Some(CardOrderStatus::AwaitingPayment), Unsettled::Stopped) => {
            format!("It stays payable{until}. Once it is paid, {land}")
        }
        (Some(CardOrderStatus::Processing), _) => {
            format!("{why}It is paid and being delivered: {land}")
        }
        (_, _) => format!(
            "{why}It is {}. The order carries on without this command: if it is paid, {land}",
            order.status_text()
        ),
    });
    after.balance(&mut paragraphs);
    paragraphs.join("\n\n")
}

/// ` until 2026-10-15T03:33:12Z`: when an unpaid order stops being payable.
/// Nothing when the order has no deadline.
fn payable_until(order: &OrderView) -> String {
    order
        .expires_at
        .map(|deadline| format!(" until {}", format_secs(deadline)))
        .unwrap_or_default()
}

/// The URL as a QR code drawn with half-block characters, two modules per
/// row, for scanning off the terminal: light modules are the blocks, dark
/// ones the spaces, in the terminal's own colours (`black_on_white` fixes
/// those where it can). It takes the lowest error correction (a screen does
/// not smudge) and a two-module quiet zone, the least a scanner needs, so
/// that the Payment Link's code fits an 80x24 terminal with the wait's line
/// below it. `None` only if the URL is too long to encode.
fn render_qr(url: &str) -> Option<String> {
    use qrcode::render::unicode::Dense1x2;
    use qrcode::{EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L).ok()?;
    let drawn = code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .quiet_zone(false)
        .build();
    let rows: Vec<&str> = drawn.lines().collect();
    let width = rows.first().map_or(0, |row| row.chars().count());
    // A code is an odd number of modules high, and the renderer leaves the
    // half row below the last one empty: made light, as the quiet zone is,
    // so no dark stripe comes between the code and its edge.
    let last = rows.len().checked_sub(1)?;
    let edge = "█".repeat(width + 4);
    let body = rows.iter().enumerate().map(|(y, row)| {
        let row = if y == last {
            row.replace('▀', "█").replace(' ', "▄")
        } else {
            (*row).to_owned()
        };
        format!("██{row}██")
    });
    Some(
        std::iter::once(edge.clone())
            .chain(body)
            .chain(std::iter::once(edge))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The drawn code, black on white whatever the terminal's theme: every row
/// set to white (256-colour palette entry 231) on black (entry 16) and reset
/// at its end. Left in the terminal's own colours, the code is at the mercy
/// of the theme: a light theme inverts it, and a dark theme's background is
/// often a grey rather than black, and many phone cameras read neither.
/// Entries 16 and 231 are in the fixed colour cube, which themes do not
/// remap as they do the first 16 colours, so the result is the same on all
/// of them. For a terminal without colour support the caller prints the
/// plain drawing, which is what every terminal got before: the worst case
/// is no worse than today. The width and height to fit on the screen are
/// those of the plain drawing; measure that, not this.
fn black_on_white(drawing: &str) -> String {
    drawing
        .lines()
        .map(|row| format!("\x1b[38;5;231;48;5;16m{row}\x1b[0m"))
        .collect::<Vec<_>>()
        .join("\n")
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
            status: order.status_tag(),
            cycles_delivered: order.cycles_delivered.map(|c| c.to_string()),
            paid: order.paid,
            refunded: order.refunded,
            detail: order.detail.as_deref(),
            balance: balance.map(|b| b.to_string()),
        }),
        Output::Quiet => {
            println!("{}", order.status_tag());
            Ok(())
        }
        Output::Human => Ok(()),
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
    use icp_app::context::NetworkSelection;
    use icp_project::host::EnvironmentSelection;

    use super::*;
    use crate::cyclepay::interface::CYCLES_GATEWAY_CID;

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

    fn order_with(status: Option<CardOrderStatus>, expires_at: Option<u64>) -> OrderView {
        OrderView {
            id: "co-1".into(),
            status,
            amount: Money::from_minor_units(1000, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            checkout_url: None,
            expires_at,
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
            destination: None,
        }
    }

    fn selections(identity: IdentitySelection) -> TokenCommandSelections {
        TokenCommandSelections {
            environment: EnvironmentSelection::Default,
            network: NetworkSelection::Named("ic".into()),
            identity,
        }
    }

    /// A run on mainnet as the default identity, `alice`.
    fn after() -> FollowUp {
        FollowUp {
            flags: paste_flags(&selections(IdentitySelection::Default), Some("alice")),
            canister: Principal::from_text(CYCLES_GATEWAY_CID).unwrap(),
        }
    }

    /// Paragraphs with a blank line between them; commands on lines of
    /// their own, indented, after a blank line; and no backticks.
    fn assert_laid_out(message: &str) {
        let lines: Vec<&str> = message.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            if line.starts_with("    ") {
                assert!(line.starts_with("    icp "), "{message}");
                assert!(
                    lines[n - 1].is_empty() || lines[n - 1].starts_with("    "),
                    "{message}"
                );
            } else if !line.is_empty() && n > 0 {
                assert!(lines[n - 1].is_empty(), "{n}: {line}\n{message}");
            }
        }
        assert!(!message.contains('`'), "{message}");
        assert!(!message.ends_with('\n'), "{message:?}");
    }

    /// `--json` and `-q` imply `--yes` and keep the quote off stderr; `--yes`
    /// alone, or nothing, still shows it, and only `--yes` skips the asking.
    #[test]
    fn a_machine_readable_run_implies_yes() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            args: BuyArgs,
        }
        let consent_to = |flags: &[&str]| {
            let cli = <Cli as clap::Parser>::try_parse_from(
                ["icp", "--amount", "10"].iter().chain(flags),
            )
            .unwrap();
            consent(&cli.args)
        };
        // (yes, speak)
        assert_eq!(consent_to(&[]), (false, true));
        assert_eq!(consent_to(&["--yes"]), (true, true));
        assert_eq!(consent_to(&["--json"]), (true, false));
        assert_eq!(consent_to(&["--quiet"]), (true, false));
        assert_eq!(consent_to(&["-q"]), (true, false));
        assert_eq!(consent_to(&["--yes", "--json"]), (true, false));
        assert_eq!(consent_to(&["-y", "-q"]), (true, false));
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
        let qr = render_qr(url).unwrap();
        let lines: Vec<&str> = qr.lines().collect();
        // Square-ish, within 80x24 with the wait's line below, and the block
        // characters are the only ink.
        let width = lines[0].chars().count();
        assert!(width > 20 && width <= 45, "{width}");
        assert!(lines.len() <= 23, "{}", lines.len());
        assert!(lines.iter().all(|l| l.chars().count() == width));
        assert!(
            qr.chars()
                .all(|c| matches!(c, '█' | '▀' | '▄' | ' ' | '\n')),
            "{qr}"
        );
        // A light quiet zone all round: full rows above and below, and two
        // blocks at either end of every row. The row above the bottom edge
        // is light in its lower half, where the renderer left the half row
        // below the code's odd height empty.
        assert!(lines[0].chars().all(|c| c == '█'), "{qr}");
        assert!(lines[lines.len() - 1].chars().all(|c| c == '█'), "{qr}");
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("██") && l.ends_with("██")),
            "{qr}"
        );
        assert!(!lines[lines.len() - 2].contains([' ', '▀']), "{qr}");
        // Far beyond a QR code's capacity.
        assert!(render_qr(&"x".repeat(10_000)).is_none());
    }

    #[test]
    fn the_qr_code_is_coloured_black_on_white_row_by_row() {
        let url = "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00?client_reference_id=o_co-1-0000abcd&prefilled_amount=1000";
        let plain = render_qr(url).unwrap();
        let coloured = black_on_white(&plain);
        const SET: &str = "\x1b[38;5;231;48;5;16m";
        const RESET: &str = "\x1b[0m";
        let rows: Vec<&str> = coloured.lines().collect();
        assert_eq!(rows.len(), plain.lines().count());
        // Every row is set and reset on its own, so nothing bleeds into the
        // line after the code, and stripped of the escapes it is the plain
        // drawing again.
        assert!(
            rows.iter()
                .all(|row| row.starts_with(SET) && row.ends_with(RESET)),
            "{coloured:?}"
        );
        let stripped: Vec<String> = rows
            .iter()
            .map(|row| {
                row.strip_prefix(SET)
                    .and_then(|row| row.strip_suffix(RESET))
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(stripped.join("\n"), plain);
        assert!(!stripped.iter().any(|row| row.contains('\x1b')));
    }

    #[test]
    fn only_a_delivery_ends_well() {
        let order = |status| OrderView {
            paid: Some(Money::from_minor_units(1000, Currency::Usd)),
            refunded: Some(Money::from_minor_units(1000, Currency::Usd)),
            ..order_with(status, None)
        };
        let names = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        assert!(ending(&order(Some(CardOrderStatus::Delivered)), names, &after()).is_ok());
        for (status, says) in [
            (Some(CardOrderStatus::Refunded), "10.00 USD was refunded"),
            (Some(CardOrderStatus::Refunding), "is being refunded"),
            (Some(CardOrderStatus::RefundOwed), "owes you a refund"),
            (Some(CardOrderStatus::NeedsReview), "Keep the order id"),
        ] {
            let err = ending(&order(status), names, &after())
                .unwrap_err()
                .to_string();
            assert!(err.contains(says), "{status:?}: {err}");
            assert_laid_out(&err);
        }
        // An order an operator handles comes with the command that reads it
        // back, as the identity and on the network the run used.
        let err = ending(&order(Some(CardOrderStatus::RefundOwed)), names, &after())
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "order co-1 could not be delivered: CyclePay owes you a refund, which an operator \
             makes by hand.\n\
             \n\
             Keep the order id. CyclePay's site shows how it ends under My purchases, and so \
             does:\n\
             \n    icp canister call 2bwus-iiaaa-aaaac-bee3a-cai order_status --query \
             --identity alice -n ic '(\"co-1\")'"
        );
    }

    /// `unsettled`, as said of an order on CyclePay's gateway.
    fn said(order: &OrderView, how: Unsettled) -> String {
        let names = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        unsettled(order, how, names, &after())
    }

    /// A wait that stops, gives up or loses contact says once where the
    /// order stands: payable only when it is known to be unpaid, paid when
    /// it is, and both when contact was lost and it may have been paid
    /// since. Every one ends with the command that shows the cycles, on a
    /// line of its own, as the identity and on the network the run used.
    #[test]
    fn an_unsettled_order_is_said_once_where_it_stands() {
        let unpaid = OrderView {
            id: "co-12-20d30d24771ff92787f41d95b6764a61".into(),
            ..order_with(Some(CardOrderStatus::AwaitingPayment), Some(1_700_000_000))
        };
        assert_eq!(
            said(&unpaid, Unsettled::Stopped),
            "\
stopped waiting for order co-12-20d30d24771ff92787f41d95b6764a61.

It stays payable until 2023-11-14T22:13:20Z. Once it is paid, its cycles land on your identity's cycles-ledger account. To see them:

    icp cycles balance --identity alice -n ic"
        );
        assert_eq!(
            said(&unpaid, Unsettled::TimedOut),
            "\
gave up waiting for order co-12-20d30d24771ff92787f41d95b6764a61.

No payment arrived in an hour. It stays payable until 2023-11-14T22:13:20Z; once it is paid, its cycles land on your identity's cycles-ledger account. To see them:

    icp cycles balance --identity alice -n ic"
        );
        assert_eq!(
            said(&unpaid, Unsettled::ContactLost),
            "\
gave up waiting for order co-12-20d30d24771ff92787f41d95b6764a61.

CyclePay stopped answering. The order carries on without this command. Unpaid, it stays payable until 2023-11-14T22:13:20Z; once it is paid, its cycles land on your identity's cycles-ledger account. To see them:

    icp cycles balance --identity alice -n ic"
        );

        let delivering = order_with(Some(CardOrderStatus::Processing), None);
        let text = said(&delivering, Unsettled::TimedOut);
        assert!(
            text.contains("An hour passed. It is paid and being delivered: its cycles land"),
            "{text}"
        );
        assert!(!text.contains("payable"), "{text}");
        // Paid is said once: no "once it is paid" after "is paid".
        assert_eq!(text.matches("paid").count(), 1, "{text}");

        for how in [
            Unsettled::Stopped,
            Unsettled::TimedOut,
            Unsettled::ContactLost,
        ] {
            let text = said(&order_with(None, None), how);
            assert!(text.contains("does not know"), "{text}");
            assert!(!text.contains("payable"), "{text}");
            assert!(
                text.ends_with(":\n\n    icp cycles balance --identity alice -n ic"),
                "{text}"
            );
            assert_laid_out(&text);
            assert_laid_out(&said(&unpaid, how));
        }
        // Another gateway is not called CyclePay when it stops answering.
        let names = Names::of(&gateway_at("ryjl3-tyaaa-aaaaa-aaaba-cai"));
        let text = unsettled(&unpaid, Unsettled::ContactLost, names, &after());
        assert!(text.contains("The gateway stopped answering."), "{text}");
        assert!(!text.contains("CyclePay"), "{text}");
    }

    /// The follow-up commands carry the run's identity (the default named,
    /// since only the identity that made an order can read it), network and
    /// environment, quoted for every shell, and leave a blank with a note
    /// where no quoting works.
    #[test]
    fn the_follow_up_commands_carry_the_selections() {
        let flags = |identity, name: Option<&str>| paste_flags(&selections(identity), name).text;
        assert_eq!(
            flags(IdentitySelection::Default, Some("alice")),
            " --identity alice -n ic"
        );
        assert_eq!(flags(IdentitySelection::Default, None), " -n ic");
        let named = |name: &str| IdentitySelection::Named(name.to_owned());
        assert_eq!(
            flags(named("-dash"), Some("-dash")),
            " --identity=-dash -n ic"
        );
        assert_eq!(
            flags(named("o'neil"), Some("o'neil")),
            " --identity \"o'neil\" -n ic"
        );
        let environment = TokenCommandSelections {
            environment: EnvironmentSelection::Named("ic".into()),
            network: NetworkSelection::Default,
            identity: IdentitySelection::Default,
        };
        assert_eq!(
            paste_flags(&environment, Some("alice")).text,
            " --identity alice -e ic"
        );

        let order = order_with(Some(CardOrderStatus::NeedsReview), None);
        let names = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        let unquotable = FollowUp {
            flags: paste_flags(&selections(named("o'neil $x")), Some("o'neil $x")),
            ..after()
        };
        let err = ending(&order, names, &unquotable).unwrap_err().to_string();
        assert!(
            err.contains(
                "\n    icp canister call 2bwus-iiaaa-aaaac-bee3a-cai order_status --query \
                 --identity <your identity> -n ic '(\"co-1\")'\n"
            ),
            "{err}"
        );
        assert!(
            err.ends_with(
                "\n\nFill in <your identity> yourself: no quoting of it works in every shell."
            ),
            "{err}"
        );
        assert_laid_out(&err);
        // The balance command carries the same blank and note.
        let mut paragraphs = Vec::new();
        unquotable.balance(&mut paragraphs);
        assert_eq!(
            paragraphs,
            [
                "    icp cycles balance --identity <your identity> -n ic",
                "Fill in <your identity> yourself: no quoting of it works in every shell."
            ]
        );
    }

    /// A delivery is told against the place the order says, whichever it is.
    #[test]
    fn a_delivery_names_where_the_cycles_went() {
        let principal = |text| Principal::from_text(text).unwrap();
        let order = |destination| OrderView {
            id: "co-1".into(),
            status: Some(CardOrderStatus::Delivered),
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
        let names = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        assert_eq!(stale_rate_warning(&rate(false), names), None);
        let warning = stale_rate_warning(&rate(true), names).unwrap();
        assert!(warning.contains("for 5 hours"), "{warning}");
    }

    /// Exactly one of --amount and --cycles, said as such when neither is
    /// given; and a negative value of either refused by clap in the same
    /// words, the form with a suffix included, rather than read as a flag.
    #[test]
    fn amount_or_cycles_is_required_and_neither_may_be_negative() {
        use clap::error::ErrorKind;
        use clap::{CommandFactory, Parser};

        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            buy: BuyArgs,
        }

        let err = Cli::try_parse_from(["buy"]).err().unwrap().to_string();
        assert!(err.contains("--amount <AMOUNT>|--cycles <CYCLES>"), "{err}");
        let err = Cli::try_parse_from(["buy", "--amount", "1", "--cycles", "1t"])
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("cannot be used with"), "{err}");
        for (args, says) in [
            (
                ["buy", "--amount", "-1"],
                "invalid value '-1' for '--amount <AMOUNT>': cannot be negative: give the \
                 amount to spend, such as 10 or 12.50",
            ),
            (
                ["buy", "--amount", "-0.5"],
                "invalid value '-0.5' for '--amount <AMOUNT>': cannot be negative",
            ),
            (
                ["buy", "--cycles", "-1"],
                "invalid value '-1' for '--cycles <CYCLES>': cannot be negative: give the \
                 cycles to receive, such as 5t",
            ),
            (
                ["buy", "--cycles", "-5t"],
                "invalid value '-5t' for '--cycles <CYCLES>': cannot be negative: give the \
                 cycles to receive, such as 5t",
            ),
            (
                ["buy", "--amount", "--yes"],
                "invalid value '--yes' for '--amount <AMOUNT>': give the amount to spend",
            ),
        ] {
            let err = Cli::try_parse_from(args).err().unwrap();
            assert_eq!(err.kind(), ErrorKind::ValueValidation, "{args:?}");
            assert_eq!(err.exit_code(), 2, "{args:?}");
            assert!(err.to_string().contains(says), "{args:?}: {err}");
        }
        assert_eq!(parse_cycles("5t").unwrap().get(), 5_000_000_000_000);
        assert_eq!(parse_amount("12.50").unwrap(), "12.50");
        Cli::command().debug_assert();
    }

    /// Nothing of CyclePay's, or of Stripe's, is said of another gateway.
    #[test]
    fn another_gateway_is_not_called_cyclepay() {
        let names = Names::of(&gateway_at("ryjl3-tyaaa-aaaaa-aaaba-cai"));
        let stale = Rate {
            xdr_usd_x10000: 13_520,
            stale: true,
            age: None,
        };
        let warning = stale_rate_warning(&stale, names).unwrap();
        assert!(
            warning.starts_with("The gateway has not refreshed"),
            "{warning}"
        );
        assert!(!names.page.contains("Stripe"), "{}", names.page);
        let order = |status| OrderView {
            id: "co-1".into(),
            status: Some(status),
            amount: Money::from_minor_units(1000, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            checkout_url: None,
            expires_at: None,
            paid: None,
            cycles_delivered: None,
            refunded: None,
            detail: None,
            destination: None,
        };
        for status in [CardOrderStatus::RefundOwed, CardOrderStatus::NeedsReview] {
            let err = ending(&order(status), names, &after())
                .unwrap_err()
                .to_string();
            assert!(!err.contains("CyclePay"), "{err}");
            assert!(
                err.contains(
                    "\n\nKeep the order id. To see how it ends:\n\n    icp canister call "
                ),
                "{err}"
            );
        }
        let cyclepay = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        assert_eq!(cyclepay.page, "Stripe's page");
        assert!(
            stale_rate_warning(&stale, cyclepay)
                .unwrap()
                .starts_with("CyclePay has not refreshed")
        );
    }

    /// CyclePay's ids are `co-<n>-<32 hex>`, 37 to 41 characters today: the
    /// live line names the order by its number, and fits 80 columns.
    #[test]
    fn the_live_line_names_the_order_by_its_number_and_fits_80_columns() {
        let id = "co-1234-0123456789abcdef0123456789abcdef";
        assert_eq!(order_number(id), "co-1234");
        assert_eq!(order_number("co-6-5e64b574"), "co-6");
        assert_eq!(order_number("o1"), "o1");
        for phase in [PurchasePhase::AwaitingPayment, PurchasePhase::Delivering] {
            let line = format!("[order {}] ✷ {}", order_number(id), live_message(phase));
            assert!(
                line.chars().count() <= 80,
                "{} columns: {line}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn timestamps_and_ages_read_well() {
        assert_eq!(format_secs(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_age(Duration::from_secs(90)), "1 minute");
        assert_eq!(format_age(Duration::from_secs(5 * 3600)), "5 hours");
        assert_eq!(format_age(Duration::from_secs(3 * 86_400)), "3 days");
    }
}
