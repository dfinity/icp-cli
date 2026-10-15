use std::io::{self, IsTerminal, stderr, stdin, stdout};
use std::time::Duration;

use anyhow::{anyhow, bail};
use candid::Principal;
use clap::{ArgGroup, Args};
use dialoguer::Confirm;
use dialoguer::console::{Term, colors_enabled_stderr};
use icp_app::context::{
    Context, GetAgentError, GetAgentForEnvError, GetAgentForNetworkError, GetNetworkError,
};
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
use tokio::select;
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
    Rate, Target, WaitEvent, WaitOptions,
};
use crate::render::{ProgressManager, ProgressManagerSettings};
use crate::shortfall::Message;

/// Buy cycles with a card.
///
/// Creates an order on a cycles gateway canister as the current identity,
/// prints the page to pay on, with a QR code to scan on a terminal (and opens
/// it in a browser when run from a terminal, but not over SSH or on CI), then
/// waits for the cycles to land on the identity's own cycles-ledger account.
/// Fund a canister from there with `icp canister top-up`. Say either how much
/// to spend (--amount, in the currency given by --currency) or how many cycles
/// to receive (--cycles). The quote shows the gateway's card fee and what the
/// amount is estimated to buy. The default gateway is CyclePay's, on mainnet
/// (-n ic, or an environment on mainnet such as -e ic), which accepts USD
/// only.
///
/// The wait asks the gateway every 3 seconds for up to an hour. It says so
/// when the gateway has not answered for 9 seconds, and gives up after five
/// minutes without word of the order. Stopping it, with Ctrl-C or by the hour
/// running out, does not stop the order: an unpaid order stays payable until
/// its deadline, and a paid one is still delivered. With --json, the last
/// line says how the wait ended.
///
/// Exactly one of --amount or --cycles must be given.
#[derive(Debug, Args)]
#[command(
    group(ArgGroup::new("what").required(true).args(["amount", "cycles"])),
    after_long_help = "\
Examples:

    # Spend 10 USD on cycles for the current identity, on mainnet
    icp cycles buy --amount 10 -n ic

    # Ask for 5 trillion cycles, and pay the least whose estimate covers them
    icp cycles buy --cycles 5t -n ic
"
)]
pub(crate) struct BuyArgs {
    /// Amount to spend, as a decimal (e.g. 10 or 12.50), in the currency given by --currency
    #[arg(
        long,
        value_name = "AMOUNT",
        allow_hyphen_values = true,
        value_parser = parse_amount
    )]
    pub(crate) amount: Option<String>,

    /// Cycles to receive: the amount to pay is the least whose estimate, at the current rate with
    /// the card fee taken out, covers them. The cycles are minted at the rate in force when the
    /// payment lands, so what arrives can differ slightly from the estimate.
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

    /// Skip confirmation prompts
    #[arg(long, short)]
    pub(crate) yes: bool,

    /// Do not open the payment page in a browser
    #[arg(long)]
    pub(crate) no_open: bool,

    /// Print the payment page's link and return without waiting for the order to complete
    #[arg(long)]
    pub(crate) no_wait: bool,

    /// Output command results as JSON: one object once the order is made, and, unless --no-wait,
    /// one when the wait ends, saying how
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
    purchase::check_gateway_id(canister).map_err(reported)?;
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
        // Outside a project the one network there is, by name, is ic: the
        // error, which looks for the name in a project, would not say so.
        Err(GetAgentError::GetAgentForNetwork {
            source:
                GetAgentForNetworkError::GetNetwork {
                    source: GetNetworkError::NetworkNotFound { name },
                },
        }) if cyclepay && !ctx.host.project.exists().await.unwrap_or(true) => {
            bail!(
                "{}",
                paragraphs(&[
                    &format!("there is no network named '{name}' outside a project."),
                    MAINNET_HINT
                ])
            )
        }
        // No network to reach: none named outside a project, one named that
        // the project does not have, or the one chosen is a local one that
        // is not running. CyclePay's gateway is on mainnet only, which is
        // always reachable.
        Err(
            error @ (GetAgentError::NoProjectOrNetwork
            | GetAgentError::GetAgentForEnv {
                source: GetAgentForEnvError::NetworkAccess { .. },
            }
            | GetAgentError::GetAgentForNetwork {
                source:
                    GetAgentForNetworkError::NetworkAccess { .. }
                    | GetAgentForNetworkError::GetNetwork {
                        source: GetNetworkError::NetworkNotFound { .. },
                    },
            }),
        ) if cyclepay => {
            bail!("{}", paragraphs(&[&format!("{error}."), MAINNET_HINT]))
        }
        Err(error) => return Err(error.into()),
    };
    // CyclePay's gateway is on mainnet only: refuse any other network before
    // asking it anything.
    purchase::check_network(canister, &agent.read_root_key()).map_err(reported)?;
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
    let gateway = purchase::connect(calls, canister).await.map_err(reported)?;
    let output = Output::of(args);

    let identity = identity_name(ctx, &selections.identity).await;
    let flags = paste_flags(&selections, identity.as_deref());
    let (order, quote, interrupt) = create_order(
        calls,
        &gateway,
        buyer,
        identity.as_deref(),
        target,
        args,
        &flags,
        &mut TerminalPrompt,
    )
    .await?;
    let after = FollowUp { flags, canister };
    announce_payable(&order, &quote, &gateway, buyer, output, args.no_open);

    if args.no_wait {
        if output == Output::Human {
            let mut message = Message::default();
            message.prose(
                "Not waiting. Once the order is paid, its cycles land on your identity's \
                 cycles-ledger account. To see them:",
            );
            after.balance(&mut message);
            info!("{}", message.text());
        }
        return Ok(());
    }

    wait_and_report(
        calls, &agent, &gateway, buyer, order, output, ctx.debug, interrupt, &after,
    )
    .await
}

/// What the gateway is called in prose. CyclePay's own is named, with
/// Stripe, which takes its card payments; nothing of either is said of any
/// other gateway, which may sell some other way.
#[derive(Clone, Copy)]
struct Names {
    /// At the start of a sentence: `CyclePay`, `The gateway`.
    subject: &'static str,
    /// Within one: `CyclePay`, `the gateway`.
    name: &'static str,
    /// The page a card is paid on.
    page: &'static str,
    cyclepay: bool,
}

impl Names {
    fn of(gateway: &Gateway) -> Self {
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

/// `paragraphs` as an error's message: each wrapped to 80 columns on lines
/// of its own.
fn paragraphs(paragraphs: &[&str]) -> String {
    let mut message = Message::default();
    for paragraph in paragraphs {
        message.prose(paragraph);
    }
    message.text()
}

/// `error` as the command reports it: its message wrapped to 80 columns
/// when it is the whole story, as it is for most; one with a cause keeps
/// its chain, for anyhow to print beneath it.
fn reported(error: CyclesPurchaseError) -> anyhow::Error {
    if std::error::Error::source(&error).is_some() {
        return error.into();
    }
    anyhow!("{}", paragraphs(&[&error.to_string()]))
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

/// `commands`, set apart in `message`, and a word on any blank in `flags`
/// that the reader fills in.
fn commands_with(message: &mut Message, commands: &[String], flags: &PasteFlags) {
    message.commands(commands);
    for note in flags.notes() {
        message.prose(&note);
    }
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
    fn balance(&self, message: &mut Message) {
        let command = format!("icp cycles balance{}", self.flags.text);
        commands_with(message, &[command], &self.flags);
    }

    /// The command that reads order `id` back.
    fn order_status(&self, message: &mut Message, id: &str) {
        let arg = shell_word(&format!("(\"{}\")", id.escape_default()))
            .unwrap_or_else(|| "'(\"<order id>\")'".to_owned());
        let command = format!(
            "icp canister call {} {ORDER_STATUS_METHOD} --query{} {arg}",
            self.canister, self.flags.text
        );
        commands_with(message, &[command], &self.flags);
    }
}

/// Quote, confirm, create. Returns the order with the quote it was placed on,
/// and what the wait is to stop on.
#[allow(clippy::too_many_arguments)]
async fn create_order(
    calls: &dyn CanisterCalls,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
    target: Target,
    args: &BuyArgs,
    flags: &PasteFlags,
    prompt: &mut dyn Prompt,
) -> Result<(OrderView, Quote, Interrupt), anyhow::Error> {
    let quote = purchase::quote_target(calls, gateway, target)
        .await
        .map_err(reported)?;
    // A script that said --yes reads the figures off --json; -q asks for
    // nothing but the link.
    let speak = !args.yes || !(args.quiet || args.json);
    let interrupt = confirm(
        &quote, target, gateway, buyer, identity, args.yes, speak, prompt,
    )?;
    // Caught from here on, prompt or not: a stop while the order is made
    // must not end the process before it can say the order may exist.
    let mut interrupt = interrupt.caught()?;
    let names = Names::of(gateway);
    let placed = purchase::place_order(calls, gateway, quote.amount, interrupt.received()).await;
    match placed {
        Ok(order) => Ok((order, quote, interrupt)),
        Err(CyclesPurchaseError::OrderRefused { error }) => {
            bail!("{} refused the order: {error}", names.subject)
        }
        Err(CyclesPurchaseError::InterruptedPlacing { .. }) => {
            // A terminal echoes the ^C where the cursor is: the error starts
            // a line of its own.
            if stderr().is_terminal() {
                eprintln!();
            }
            bail!("{}", not_known(&quote, names, flags))
        }
        Err(error) => Err(reported(error)),
    }
}

/// What to say when the command was stopped before the gateway answered
/// whether it made the order: that it may or may not have, and how to find
/// out. CyclePay hands an open order back to the identity that asks again
/// for the same amount within the hour, rather than making a second one.
fn not_known(quote: &Quote, names: Names, flags: &PasteFlags) -> String {
    let mut message = Message::default();
    message.prose(&format!(
        "stopped before {} answered; the order may or may not exist.",
        names.name
    ));
    if names.cyclepay {
        message.prose(
            "Nothing is charged unless it is paid. To find out, run the command again within \
             the hour with the same amount: CyclePay hands back the open order if it made one, \
             rather than making another.",
        );
        let again = format!(
            "icp cycles buy --amount {}{}",
            quote.amount.to_decimal_string(),
            flags.text
        );
        commands_with(&mut message, &[again], flags);
    } else {
        message.prose(
            "Nothing is charged unless it is paid, and an unpaid order lapses at its deadline.",
        );
    }
    message.text()
}

/// The answer to the confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Yes,
    No,
    /// Ctrl-C, once it is caught.
    Interrupted,
}

/// Where the confirmation is asked: the terminal, or a stand-in under test.
trait Prompt {
    /// Whether stdin, which the answer is read on, and stderr, which the
    /// question is drawn on, are terminals.
    fn terminals(&self) -> (bool, bool);

    /// Ask `question`.
    fn ask(&mut self, question: &str) -> Result<Answer, dialoguer::Error>;

    /// Leave the question unanswered after Ctrl-C: show the cursor it hid,
    /// and end its line.
    fn abandon(&mut self);
}

/// The prompt on the terminal.
struct TerminalPrompt;

impl Prompt for TerminalPrompt {
    fn terminals(&self) -> (bool, bool) {
        (stdin().is_terminal(), stderr().is_terminal())
    }

    fn ask(&mut self, question: &str) -> Result<Answer, dialoguer::Error> {
        match Confirm::new()
            .with_prompt(question)
            .default(false)
            .interact()
        {
            Ok(true) => Ok(Answer::Yes),
            Ok(false) => Ok(Answer::No),
            // Ctrl-C, read as an answer: the prompt leaves its cursor hidden
            // and its line open.
            Err(dialoguer::Error::IO(e)) if e.kind() == io::ErrorKind::Interrupted => {
                Ok(Answer::Interrupted)
            }
            Err(e) => Err(e),
        }
    }

    fn abandon(&mut self) {
        let _ = Term::stderr().show_cursor();
        eprintln!();
    }
}

/// Show what is about to happen and ask. Without a terminal to ask on there
/// is nobody to ask, so `--yes` is required. Returns what the wait is to
/// stop on.
#[allow(clippy::too_many_arguments)]
fn confirm(
    quote: &Quote,
    target: Target,
    gateway: &Gateway,
    buyer: Principal,
    identity: Option<&str>,
    yes: bool,
    speak: bool,
    prompt: &mut dyn Prompt,
) -> Result<Interrupt, anyhow::Error> {
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
        return Ok(Interrupt::later());
    }

    if yes {
        info!("Proceeding without confirmation (--yes).");
        return Ok(Interrupt::later());
    }
    let (stdin_is_terminal, stderr_is_terminal) = prompt.terminals();
    if let Some(why) = cannot_prompt(stdin_is_terminal, stderr_is_terminal) {
        bail!("{}", paragraphs(&[why]));
    }
    // Said apart from the question, which stays short: a question wider
    // than the terminal is drawn again on every keypress.
    info!("Nothing is charged until you pay on {}.", names.page);
    // Caught before the question is asked, so that Ctrl-C is an answer to
    // it rather than the end of the process.
    let interrupt = Interrupt::catch()?;
    match prompt.ask("Create this order?")? {
        Answer::Yes => Ok(interrupt),
        Answer::No => bail!("no order created"),
        Answer::Interrupted => {
            prompt.abandon();
            bail!("no order created")
        }
    }
}

/// Why the confirmation cannot be asked, if it cannot: the answer is read on
/// stdin and the question drawn on stderr, so both must be a terminal.
fn cannot_prompt(stdin_is_terminal: bool, stderr_is_terminal: bool) -> Option<&'static str> {
    match (stdin_is_terminal, stderr_is_terminal) {
        (true, true) => None,
        (true, false) => Some(
            "Refusing to create an order without confirmation: the prompt is drawn on stderr, \
             which is not a terminal here. Use --yes to proceed.",
        ),
        (false, _) => Some(
            "Refusing to create an order without confirmation in a non-interactive context. \
             Use --yes to proceed.",
        ),
    }
}

/// Ctrl-C, or a stop signal, for the order's creation, the wait and the
/// balance read after it to stop on.
///
/// Catching Ctrl-C is what lets the prompt read it as an answer: left alone,
/// it ends the process with the prompt's cursor hidden. Once caught it stays
/// caught for the life of the process, so everything after the prompt must
/// race its work against [`Interrupt::received`]; the same listeners are
/// handed on, so that a stop sent between two steps is not lost.
struct Interrupt {
    #[cfg(unix)]
    caught: Option<(tokio::signal::unix::Signal, tokio::signal::unix::Signal)>,
}

impl Interrupt {
    /// Nothing caught yet.
    fn later() -> Self {
        Self {
            #[cfg(unix)]
            caught: None,
        }
    }

    /// Catch Ctrl-C and SIGTERM from now on. Only on Unix: Windows hands a
    /// caught Ctrl-C to no reader, so the prompt there is left as it is, and
    /// the steps after it listen as icp-cli's other waits do.
    fn catch() -> io::Result<Self> {
        Self::later().caught()
    }

    /// Caught from now on, if not already.
    fn caught(self) -> io::Result<Self> {
        #[cfg(unix)]
        if self.caught.is_none() {
            use tokio::signal::unix::{SignalKind, signal};
            return Ok(Self {
                caught: Some((
                    signal(SignalKind::interrupt())?,
                    signal(SignalKind::terminate())?,
                )),
            });
        }
        Ok(self)
    }

    /// The next stop: one sent since the last was taken counts.
    async fn received(&mut self) {
        #[cfg(unix)]
        if let Some((interrupt, terminate)) = &mut self.caught {
            select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
        stop_signal().await
    }
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
    // What was asked for, not a promise: the estimate below is what the
    // gateway's published arithmetic says the price buys at today's rate.
    if let Target::Receive { cycles, .. } = target {
        lines.push(format!("Wanted:   {}", format_cycles(cycles)));
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
fn announce_payable(
    order: &OrderView,
    quote: &Quote,
    gateway: &Gateway,
    buyer: Principal,
    output: Output,
    no_open: bool,
) {
    let Some(url) = order.checkout_url.as_deref() else {
        warn!(
            "Order {} is {} but has no payment page",
            order.id,
            order.status_text()
        );
        return;
    };
    match output {
        Output::Json => {
            if let Err(e) = print_json(&JsonOrderCreated {
                order_id: &order.id,
                gateway: gateway.canister.to_text(),
                buyer: buyer.to_text(),
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
                    Ok(()) => info!("Opened the payment page in your browser."),
                    Err(e) => warn!("Could not open a browser ({e}); open the URL above yourself."),
                }
            }
            // A phone camera is the usual way to reach a card from a terminal;
            // the code is for eyes, so it only goes where there are some.
            if stderr().is_terminal() {
                let (rows, columns) = Term::stderr().size();
                show_qr(url, usize::from(columns), usize::from(rows));
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
    mut interrupt: Interrupt,
    after: &FollowUp,
) -> Result<(), anyhow::Error> {
    let names = Names::of(gateway);
    let id = order.id.clone();
    let made = order.clone();
    // The live line is for a person at a terminal. Anyone else reading
    // stderr as it comes (an agent, a log, the debug log a spinner would
    // interleave with) is told in lines instead; -q asks for nothing but the
    // link and the end.
    let live = output == Output::Human && stderr().is_terminal() && !debug;
    let progress = live.then(|| ProgressManager::new(ProgressManagerSettings { hidden: false }));
    let prefix = format!("[order {}]", order_number(&id));
    let bar = progress.as_ref().map(|progress| {
        let bar = progress.create_independent_progress_bar();
        bar.set_prefix(prefix.clone());
        bar
    });
    if !live && output != Output::Quiet {
        info!("If this command is stopped, the order carries on without it.");
    }
    let mut phase = PurchasePhase::AwaitingPayment;
    let mut note: Option<String> = None;
    let mut quiet = false;
    let on_event = |event: WaitEvent| {
        match &event {
            WaitEvent::Order {
                phase: entered,
                note: noted,
            } => {
                phase = *entered;
                note = noted.as_deref().map(on_one_row);
            }
            WaitEvent::ContactLost => quiet = true,
            WaitEvent::ContactRestored => quiet = false,
        }
        let showing = if quiet {
            Showing::NoAnswer
        } else {
            Showing::Phase(phase)
        };
        match &bar {
            Some(bar) => {
                let (_, columns) = Term::stderr().size();
                bar.set_message(live_message(
                    showing,
                    note.as_deref(),
                    &prefix,
                    usize::from(columns),
                ));
            }
            None if output != Output::Quiet => match event {
                WaitEvent::ContactRestored => {
                    info!("[order {id}] {} is answering again.", names.subject)
                }
                _ => info!("[order {id}] {}", line_message(showing, note.as_deref())),
            },
            None => {}
        }
    };
    let options = WaitOptions::default();
    let wait = purchase::wait_for_order(
        calls,
        gateway,
        &id,
        Some(order),
        &options,
        interrupt.received(),
        on_event,
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

    match result {
        Ok(order) => report_end(agent, buyer, &order, output, names, after, &mut interrupt).await,
        // The wait ended with the order unsettled. It carries on without
        // the command, which says once where it stands, as the error it
        // ends with.
        Err(CyclesPurchaseError::Interrupted { order }) => {
            print_final(&order, None, output, WaitEnd::Interrupted)?;
            bail!("{}", unsettled(&order, Unsettled::Stopped, names, after))
        }
        Err(CyclesPurchaseError::WaitTimedOut { order }) => {
            print_final(&order, None, output, WaitEnd::TimedOut)?;
            bail!("{}", unsettled(&order, Unsettled::TimedOut, names, after))
        }
        Err(CyclesPurchaseError::ContactLost { order, source, .. }) => {
            print_final(&order, None, output, WaitEnd::ContactLost)?;
            Err(anyhow::Error::new(*source).context(unsettled(
                &order,
                Unsettled::ContactLost,
                names,
                after,
            )))
        }
        Err(error @ CyclesPurchaseError::OrderNotFound { .. }) => {
            let unknown = OrderView {
                status: None,
                ..made
            };
            print_final(&unknown, None, output, WaitEnd::NotFound)?;
            bail!("{}", paragraphs(&[&error.to_string()]))
        }
        Err(error) => Err(reported(error)),
    }
}

/// What the live line is about: a phase of the order, or the gateway not
/// answering while the wait keeps trying.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Showing {
    Phase(PurchasePhase),
    NoAnswer,
}

/// How a wait ended other than in a delivery.
enum Ended {
    /// Settled, but not delivered.
    Order(OrderView),
    Error(CyclesPurchaseError),
}

/// The live line's message: the longest of a few that fits a terminal of
/// `columns` after `prefix` and the spinner, so that none is cut short,
/// where the cut could turn the Ctrl-C hint into its opposite. The widest
/// is 59 columns, which with `[order co-<n>] ✷ ` fits 80 up to an order
/// number of 6 digits.
///
/// The gateway's `note` on the order, if any, follows the phase's shortest
/// message, cut with `…` to fit: a delivery that waits on something says
/// what.
fn live_message(showing: Showing, note: Option<&str>, prefix: &str, columns: usize) -> String {
    let choices: &[&str] = match showing {
        Showing::Phase(PurchasePhase::AwaitingPayment) => &[
            "Waiting for payment; Ctrl-C stops waiting, not the order",
            "Waiting for payment; Ctrl-C to stop",
            "Waiting for payment",
        ],
        Showing::Phase(PurchasePhase::PaymentPending) => &[
            "Payment pending; waiting for it to settle",
            "Payment pending",
        ],
        Showing::Phase(PurchasePhase::Delivering) => &[
            "Payment received; delivering cycles...",
            "Delivering cycles...",
        ],
        Showing::NoAnswer => &[
            "No answer from the gateway, retrying; Ctrl-C stops waiting",
            "No answer from the gateway, retrying",
            "Retrying",
        ],
    };
    let shortest = choices[choices.len() - 1];
    // The prefix, a space, the spinner and a space go first. indicatif pads
    // the line with spaces to the terminal's width, so the message has the
    // rest of the row, to its last column: any longer, and it would wrap
    // onto a second row.
    let room = columns.saturating_sub(prefix.chars().count() + 3);
    if let (Showing::Phase(phase), Some(note)) = (showing, note) {
        let stem = noted(phase);
        let line = format!("{stem}: {note}");
        if line.chars().count() <= room {
            return line;
        }
        // Enough of the note to say something, and the `…` that says it
        // goes on.
        if room > stem.chars().count() + ": ".len() + 10 {
            let cut: String = line.chars().take(room - 1).collect();
            return format!("{}…", cut.trim_end());
        }
        return shortest.to_owned();
    }
    choices
        .iter()
        .copied()
        .find(|message| message.chars().count() <= room)
        .unwrap_or(shortest)
        .to_owned()
}

/// What a phase is called before the gateway's note on it.
fn noted(phase: PurchasePhase) -> &'static str {
    match phase {
        PurchasePhase::AwaitingPayment => "Waiting for payment",
        PurchasePhase::PaymentPending => "Payment pending",
        PurchasePhase::Delivering => "Delivery waits",
    }
}

/// What a line off a terminal says: not the live line's, whose Ctrl-C hint
/// is for a person at the terminal, and the gateway's note whole.
fn line_message(showing: Showing, note: Option<&str>) -> String {
    match (showing, note) {
        (Showing::NoAnswer, _) => "No answer from the gateway; retrying...".to_owned(),
        (Showing::Phase(PurchasePhase::Delivering), Some(note)) => {
            format!("Payment received; delivery waits: {note}")
        }
        (Showing::Phase(phase), Some(note)) => format!("{}: {note}", noted(phase)),
        (Showing::Phase(PurchasePhase::AwaitingPayment), None) => {
            "Waiting for payment...".to_owned()
        }
        (Showing::Phase(PurchasePhase::PaymentPending), None) => {
            "Payment pending; waiting for it to settle...".to_owned()
        }
        (Showing::Phase(PurchasePhase::Delivering), None) => {
            "Payment received; delivering cycles...".to_owned()
        }
    }
}

/// The gateway's note as it goes on one row of the live line, or one line
/// off a terminal: a line break or a tab becomes a space.
fn on_one_row(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect()
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
    interrupt: &mut Interrupt,
) -> Result<(), anyhow::Error> {
    // The buyer's balance says something only when the cycles went there:
    // an order made on CyclePay's site can top up a canister, or credit
    // another principal.
    let to_buyer = order.destination == Some(Destination::Ledger(buyer));
    let balance = if order.status == Some(CardOrderStatus::Delivered) && to_buyer {
        // A stop while the balance is read leaves it out, rather than
        // waiting on a read the network may hold for minutes.
        select! {
            read = get_raw_balance(agent, CYCLES_LEDGER_PRINCIPAL, buyer, None) => match read {
                Ok(balance) => balance.0.to_u128(),
                Err(e) => {
                    warn!("Cycles were delivered, but the balance could not be read: {e}");
                    None
                }
            },
            _ = interrupt.received() => None,
        }
    } else {
        None
    };

    print_final(order, balance, output, WaitEnd::Settled)?;

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
/// and an error that says what happened otherwise.
fn ending(order: &OrderView, names: Names, after: &FollowUp) -> Result<(), anyhow::Error> {
    let detail = order
        .detail
        .as_deref()
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    let id = &order.id;
    let mut message = Message::default();
    match order.status {
        Some(CardOrderStatus::Delivered) => return Ok(()),
        // Not an end: a refund under way can still fail, and then the
        // refund is owed and made by hand.
        Some(CardOrderStatus::Refunding) => {
            message.prose(&format!("order {id} could not be delivered{detail}."));
            message.prose(&format!(
                "A refund to your card is under way. It ends refunded, or owed a refund that {} \
                 makes by hand. To see which:",
                names.name
            ));
            after.order_status(&mut message, id);
        }
        Some(CardOrderStatus::Refunded) => {
            message.prose(&format!("order {id} could not be delivered{detail}."));
            message.prose(&format!(
                "{} was refunded to your card.",
                order
                    .refunded
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "The payment".to_owned())
            ));
        }
        Some(CardOrderStatus::RefundOwed) => {
            message.prose(&format!(
                "order {id} could not be delivered: {} owes you a refund, which an operator \
                 makes by hand{detail}.",
                names.name
            ));
            look_later(&mut message, id, names, after);
        }
        // Not a promise of a refund: a payment disputed before delivery
        // also needs review, and the dispute settles it.
        Some(CardOrderStatus::NeedsReview) => {
            message.prose(&format!(
                "order {id} needs review{detail}: your card was charged, and {} checks the order \
                 by hand (a disputed payment is settled by the dispute).",
                names.name
            ));
            look_later(&mut message, id, names, after);
        }
        Some(
            CardOrderStatus::AwaitingPayment
            | CardOrderStatus::PaymentPending
            | CardOrderStatus::Processing,
        )
        | None => message.prose(&format!("order {id} is still {}", order.status_text())),
    }
    bail!("{}", message.text())
}

/// Where to see how an order an operator handles ends: the gateway shows it
/// to the identity that made the order, and CyclePay's site to an identity
/// linked there.
fn look_later(message: &mut Message, id: &str, names: Names, after: &FollowUp) {
    if names.cyclepay {
        message.prose(
            "Keep the order id. CyclePay's site shows how it ends under My purchases, and so \
             does:",
        );
    } else {
        message.prose("Keep the order id. To see how it ends:");
    }
    after.order_status(message, id);
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
    let mut message = Message::default();
    message.prose(&match how {
        Unsettled::Stopped => format!("stopped waiting for order {id}."),
        Unsettled::TimedOut | Unsettled::ContactLost => {
            format!("gave up waiting for order {id}.")
        }
    });
    // Why, unless it was asked for.
    let why = match how {
        Unsettled::Stopped => String::new(),
        Unsettled::TimedOut => "An hour passed. ".to_owned(),
        Unsettled::ContactLost => format!("{} stopped answering. ", names.subject),
    };
    message.prose(&match (order.status, how) {
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
        (Some(CardOrderStatus::PaymentPending), _) => format!(
            "{why}Its payment has not settled yet. The order carries on without this command: \
             once the payment settles, {land}"
        ),
        (Some(CardOrderStatus::Processing), _) => {
            format!("{why}It is paid and being delivered: {land}")
        }
        (status, _) => format!(
            "{why}It is {}. The order carries on without this command: if it is paid, {land}",
            match status {
                Some(status) => status.to_string(),
                None => order.status_text(),
            }
        ),
    });
    after.balance(&mut message);
    message.text()
}

/// ` until 2026-10-15T03:33:12Z`: when an unpaid order stops being payable.
/// Nothing when the order has no deadline.
fn payable_until(order: &OrderView) -> String {
    order
        .expires_at
        .map(|deadline| format!(" until {}", format_secs(deadline)))
        .unwrap_or_default()
}

/// How a code goes on a terminal: its quiet zone across and down, in
/// modules, and whether a blank line goes above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QrLayout {
    across: usize,
    down: usize,
    blank: bool,
}

/// The most room a code `width` modules wide can have on a terminal of
/// `columns` by `rows` with the wait's line below it: four modules of quiet
/// zone across, which columns afford, else two; four down where the rows
/// allow, else two, the least a scanner needs; and a blank line above, to
/// part the code from the link, where there is one to spare. `None` if even
/// the least does not fit.
fn qr_layout(width: usize, columns: usize, rows: usize) -> Option<QrLayout> {
    let across = [4, 2]
        .into_iter()
        .find(|across| width + 2 * across <= columns)?;
    [(4, true), (4, false), (2, true), (2, false)]
        .into_iter()
        .map(|(down, blank)| QrLayout {
            across,
            down,
            blank,
        })
        .find(|layout| {
            (width + 2 * layout.down).div_ceil(2) + 1 + usize::from(layout.blank) <= rows
        })
}

/// The URL's QR code on stderr, if a terminal of `columns` by `rows` shows
/// it whole with the wait's line below it: a code wrapped across lines, or
/// partly scrolled off the screen, cannot be scanned. Else a note says how
/// large the terminal must be.
fn show_qr(url: &str, columns: usize, rows: usize) {
    // Without colour, 1.7.0's drawing, as it was.
    if !colors_enabled_stderr() {
        let Some(qr) = render_qr_plain(url) else {
            return;
        };
        let width = qr.lines().next().map_or(0, |row| row.chars().count());
        let height = qr.lines().count() + 1;
        if width > columns || height > rows {
            info!(
                "(Make the terminal {width} columns by {height} rows or larger to see the QR \
                 code.)"
            );
        } else {
            eprintln!("{qr}");
        }
        return;
    }
    let Ok(code) = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::L)
    else {
        return;
    };
    let width = code.width();
    match qr_layout(width, columns, rows) {
        Some(layout) => {
            if layout.blank {
                eprintln!();
            }
            eprintln!("{}", render_qr(&code, layout));
        }
        None => info!(
            "(Make the terminal {} columns by {} rows or larger to see the QR code.)",
            width + 4,
            (width + 4).div_ceil(2) + 1
        ),
    }
}

/// `code` drawn black on white whatever the terminal's theme, with the
/// quiet zone `layout` gives, two modules to a row: a cell whose two modules
/// are alike is a space in their colour, and one whose modules differ an
/// upper half block, its foreground the top module's colour and its
/// background the bottom one's. The half row below the code's odd height is
/// light, as the quiet zone is. The colours are the 256 colours' fixed cube
/// entries 16 (#000000) and 231 (#ffffff), which a theme does not remap as
/// it remaps the first 16; every row sets them first and resets them last,
/// so that nothing past the code takes them.
fn render_qr(code: &qrcode::QrCode, layout: QrLayout) -> String {
    let width = code.width();
    let modules = code.to_colors();
    let QrLayout { across, down, .. } = layout;
    let (columns, height) = (width + 2 * across, width + 2 * down);
    let light = |x: usize, y: usize| {
        let inside = (across..across + width).contains(&x) && (down..down + width).contains(&y);
        !inside || modules[(y - down) * width + (x - across)] == qrcode::Color::Light
    };
    let colour = |light: bool| if light { 231 } else { 16 };
    (0..height)
        .step_by(2)
        .map(|y| {
            let mut row = String::new();
            let mut set = None;
            for x in 0..columns {
                let cell = (light(x, y), light(x, y + 1));
                if set != Some(cell) {
                    row.push_str(&format!(
                        "\x1b[38;5;{};48;5;{}m",
                        colour(cell.0),
                        colour(cell.1)
                    ));
                    set = Some(cell);
                }
                row.push(if cell.0 == cell.1 { ' ' } else { '▀' });
            }
            row.push_str("\x1b[0m");
            row
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The URL as a QR code drawn with half-block characters, two modules per
/// row, in the terminal's own colours, as 1.7.0 drew it and as it is drawn
/// still without colour: light modules are the blocks, which reads right on
/// a dark theme and inverted on a light one. It takes the lowest error
/// correction and a two-module quiet zone. No escape sequence is in it.
/// `None` only if the URL is too long to encode.
fn render_qr_plain(url: &str) -> Option<String> {
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

/// Why a wait ended, for `--json`'s last line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum WaitEnd {
    /// The order reached a status the wait ends on.
    Settled,
    Interrupted,
    TimedOut,
    ContactLost,
    NotFound,
}

/// Where the order stands, for a script: one JSON object, or the status tag
/// alone once it settled. A person is told in prose by the caller.
fn print_final(
    order: &OrderView,
    balance: Option<u128>,
    output: Output,
    ended: WaitEnd,
) -> Result<(), anyhow::Error> {
    match output {
        Output::Json => print_json(&JsonOrderFinal {
            order_id: &order.id,
            status: order.status_tag(),
            ended,
            cycles_delivered: order.cycles_delivered.map(|c| c.to_string()),
            paid: order.paid,
            refunded: order.refunded,
            detail: order.detail.as_deref(),
            balance: balance.map(|b| b.to_string()),
            expires_at: order.expires_at.map(format_secs),
        }),
        Output::Quiet if ended == WaitEnd::Settled => {
            println!("{}", order.status_tag());
            Ok(())
        }
        Output::Quiet | Output::Human => Ok(()),
    }
}

/// The name behind an identity selection, for the confirmation block and
/// the commands printed to be pasted. Best effort: a name that cannot be
/// read is simply left out.
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

/// The first line `--json` prints: the order as made.
#[derive(Serialize)]
struct JsonOrderCreated<'a> {
    order_id: &'a str,
    /// The canister id of the gateway the order was placed with.
    gateway: String,
    /// The principal that placed it, whose Cycles Ledger account the cycles
    /// go to.
    buyer: String,
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

/// The last line `--json` prints, unless `--no-wait`: how the wait ended,
/// and where the order stood then. Every key is always there, `null` where
/// it does not apply.
#[derive(Serialize)]
struct JsonOrderFinal<'a> {
    order_id: &'a str,
    /// The order's status as last read, e.g. `delivered` or
    /// `awaiting_payment`; `unknown` for one this CLI does not know, or
    /// could not read.
    status: &'static str,
    /// Why the wait ended: `settled` (the order reached a status the wait
    /// ends on), `interrupted`, `timed_out`, `contact_lost` or `not_found`.
    ended: WaitEnd,
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
    /// RFC 3339: until when an unpaid order stays payable.
    expires_at: Option<String>,
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

    fn quote() -> Quote {
        Quote {
            amount: Money::from_minor_units(1000, Currency::Usd),
            fee: Money::from_minor_units(59, Currency::Usd),
            net: Money::from_minor_units(941, Currency::Usd),
            cycles: 6_959_866_863_905,
            rate: Rate {
                xdr_usd_x10000: 13_520,
                stale: false,
                age: None,
            },
        }
    }

    const BUYER: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

    #[test]
    fn the_quote_block_shows_amount_cycles_fee_and_rate() {
        let quote = quote();
        let buyer = Principal::from_text(BUYER).unwrap();
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
            ..quote()
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

    /// A Payment Link as long as the QR code's promise to fit 80x24 holds
    /// for: an order number of 5 digits and the most an order may be.
    const LONGEST_LINK: &str = "https://buy.stripe.com/6oU7sLg7t4ih4LtgwX3VC00?client_reference_id=o_co-12345-20d30d24771ff92787f41d95b6764a61&prefilled_amount=20000";

    /// The colour drawing read back as modules, light or not, a row of them
    /// for every two of the code's: (top, bottom) for each cell. Every row
    /// opens with the colour pair and ends with a reset; a cell is a space
    /// in the one colour of its two modules, or an upper half block over
    /// two.
    fn read_back(drawn: &str) -> Vec<Vec<(bool, bool)>> {
        drawn
            .lines()
            .map(|line| {
                assert!(line.starts_with("\x1b[38;5;"), "{line:?}");
                let line = line
                    .strip_suffix("\x1b[0m")
                    .expect("a reset at the row's end");
                let mut cells = Vec::new();
                for part in line.split('\x1b').filter(|part| !part.is_empty()) {
                    let (codes, blocks) = part
                        .strip_prefix('[')
                        .and_then(|part| part.split_once('m'))
                        .expect("a colour before any cell");
                    let light = |code: &str| match code {
                        "231" => true,
                        "16" => false,
                        c => panic!("unexpected colour {c}"),
                    };
                    let (fg, bg) = match codes.split(';').collect::<Vec<_>>()[..] {
                        ["38", "5", fg, "48", "5", bg] => (light(fg), light(bg)),
                        _ => panic!("unexpected {codes}"),
                    };
                    for c in blocks.chars() {
                        match c {
                            ' ' => assert_eq!(fg, bg, "a space between two colours"),
                            '▀' => assert_ne!(fg, bg, "a half block in one colour"),
                            c => panic!("unexpected {c:?}"),
                        }
                        cells.push((fg, bg));
                    }
                }
                cells
            })
            .collect()
    }

    /// 1.7.0's drawing read back the same way: light modules are the blocks.
    fn read_back_plain(drawn: &str) -> Vec<Vec<(bool, bool)>> {
        drawn
            .lines()
            .map(|line| {
                line.chars()
                    .map(|c| match c {
                        '█' => (true, true),
                        '▀' => (true, false),
                        '▄' => (false, true),
                        ' ' => (false, false),
                        c => panic!("unexpected {c:?}"),
                    })
                    .collect()
            })
            .collect()
    }

    /// Whether `rows`, read back, are the code's own modules inside a light
    /// quiet zone `across` modules wide at the sides and `down` deep above
    /// and below, with the half row below the code's odd height light too:
    /// a dark stripe there would leave a scanner no edge to find.
    fn assert_modules(
        rows: &[Vec<(bool, bool)>],
        code: &qrcode::QrCode,
        across: usize,
        down: usize,
    ) {
        let (width, modules) = (code.width(), code.to_colors());
        assert_eq!(rows.len(), (width + 2 * down).div_ceil(2));
        assert!(rows.iter().all(|row| row.len() == width + 2 * across));
        for y in 0..rows.len() * 2 {
            for x in 0..width + 2 * across {
                let (top, bottom) = rows[y / 2][x];
                let drawn_light = if y % 2 == 0 { top } else { bottom };
                let inside =
                    (across..across + width).contains(&x) && (down..down + width).contains(&y);
                let light =
                    !inside || modules[(y - down) * width + (x - across)] == qrcode::Color::Light;
                assert_eq!(
                    drawn_light, light,
                    "({x}, {y}), {across} across, {down} down"
                );
            }
        }
    }

    /// In colour the code is black on white, the modules where they are,
    /// at each quiet zone the layout can give.
    #[test]
    fn qr_draws_every_module_where_it_is() {
        use qrcode::{EcLevel, QrCode};
        let code = QrCode::with_error_correction_level(LONGEST_LINK, EcLevel::L).unwrap();
        assert_eq!(code.width() % 2, 1);
        for (across, down) in [(4, 4), (4, 2), (2, 2)] {
            let layout = QrLayout {
                across,
                down,
                blank: false,
            };
            assert_modules(&read_back(&render_qr(&code, layout)), &code, across, down);
        }
    }

    /// Without colour, the code is 1.7.0's: in the terminal's own colours,
    /// light modules as the blocks, a two-module quiet zone, and no escape
    /// sequence at all.
    #[test]
    fn qr_without_colour_is_drawn_as_before_with_no_escape_sequence() {
        use qrcode::{EcLevel, QrCode};
        let code = QrCode::with_error_correction_level(LONGEST_LINK, EcLevel::L).unwrap();
        let drawn = render_qr_plain(LONGEST_LINK).unwrap();
        assert!(!drawn.contains('\x1b'), "{drawn:?}");
        assert_modules(&read_back_plain(&drawn), &code, 2, 2);
        // Far beyond a QR code's capacity.
        assert!(render_qr_plain(&"x".repeat(10_000)).is_none());
    }

    /// The code takes four modules of quiet zone across where the columns
    /// afford them, four down where the rows do, else two, and a blank line
    /// above where one is to spare, always with the wait's line below it.
    /// The longest Payment Link's code (41 modules) fits 80x24 with two down,
    /// and 60x40 with everything.
    #[test]
    fn the_qr_layout_takes_what_room_the_terminal_has() {
        use qrcode::{EcLevel, QrCode};
        let width = QrCode::with_error_correction_level(LONGEST_LINK, EcLevel::L)
            .unwrap()
            .width();
        assert_eq!(width, 41);
        let layout = |across, down, blank| {
            Some(QrLayout {
                across,
                down,
                blank,
            })
        };
        // 45 modules high is 23 rows, and the wait's line makes 24.
        assert_eq!(qr_layout(width, 80, 24), layout(4, 2, false));
        assert_eq!(qr_layout(width, 80, 25), layout(4, 2, true));
        assert_eq!(qr_layout(width, 80, 26), layout(4, 4, false));
        assert_eq!(qr_layout(width, 80, 27), layout(4, 4, true));
        assert_eq!(qr_layout(width, 60, 40), layout(4, 4, true));
        assert_eq!(qr_layout(width, 46, 24), layout(2, 2, false));
        assert_eq!(qr_layout(width, 44, 40), None);
        assert_eq!(qr_layout(width, 80, 23), None);
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

    /// Every line within 80 columns, the first leaving room for `Error: `;
    /// commands on lines of their own, indented, with a blank line either
    /// side; and no backticks.
    fn assert_laid_out(message: &str) {
        let lines: Vec<&str> = message.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let room = if n == 0 { 80 - "Error: ".len() } else { 80 };
            assert!(
                line.chars().count() <= room || line.starts_with("    icp "),
                "{n}: {line}\n{message}"
            );
            if line.starts_with("    ") {
                assert!(line.starts_with("    icp "), "{message}");
                assert!(
                    lines[n - 1].is_empty() || lines[n - 1].starts_with("    "),
                    "{message}"
                );
            }
        }
        assert!(!message.contains('`'), "{message}");
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
            (
                Some(CardOrderStatus::Refunding),
                "A refund to your card is under way",
            ),
            (Some(CardOrderStatus::RefundOwed), "owes you a refund"),
            (Some(CardOrderStatus::NeedsReview), "Keep the order id"),
        ] {
            let err = ending(&order(status), names, &after())
                .unwrap_err()
                .to_string();
            assert!(err.replace('\n', " ").contains(says), "{status:?}: {err}");
            assert_laid_out(&err);
        }
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
It stays payable until 2023-11-14T22:13:20Z. Once it is paid, its cycles land on
your identity's cycles-ledger account. To see them:

    icp cycles balance --identity alice -n ic"
        );
        assert_eq!(
            said(&unpaid, Unsettled::TimedOut),
            "\
gave up waiting for order co-12-20d30d24771ff92787f41d95b6764a61.
No payment arrived in an hour. It stays payable until 2023-11-14T22:13:20Z; once
it is paid, its cycles land on your identity's cycles-ledger account. To see
them:

    icp cycles balance --identity alice -n ic"
        );
        let lost = said(&unpaid, Unsettled::ContactLost).replace('\n', " ");
        assert!(
            lost.starts_with("gave up waiting for order co-12-")
                && lost.contains(". CyclePay stopped answering. The order carries on"),
            "{lost}"
        );
        assert!(lost.contains("Unpaid, it stays payable until"), "{lost}");
        assert!(lost.contains("once it is paid, its cycles land"), "{lost}");

        let delivering = order_with(Some(CardOrderStatus::Processing), None);
        let text = said(&delivering, Unsettled::TimedOut).replace('\n', " ");
        assert!(text.contains("It is paid and being delivered"), "{text}");
        assert!(!text.contains("payable"), "{text}");
        // Paid is said once: no "once it is paid" after "is paid".
        assert_eq!(text.matches("paid").count(), 1, "{text}");

        let pending = order_with(Some(CardOrderStatus::PaymentPending), None);
        let text = said(&pending, Unsettled::Stopped).replace('\n', " ");
        assert!(text.contains("has not settled yet"), "{text}");
        assert!(!text.contains("payable"), "{text}");

        for how in [
            Unsettled::Stopped,
            Unsettled::TimedOut,
            Unsettled::ContactLost,
        ] {
            let text = said(&order_with(None, None), how);
            assert!(text.replace('\n', " ").contains("does not know"), "{text}");
            assert!(!text.contains("payable"), "{text}");
            assert!(
                text.ends_with(":\n\n    icp cycles balance --identity alice -n ic"),
                "{text}"
            );
            assert_laid_out(&text);
            assert_laid_out(&said(&unpaid, how));
        }
    }

    /// A delivery is told against the place the order says, whichever it is.
    #[test]
    fn a_delivery_names_where_the_cycles_went() {
        let principal = |text| Principal::from_text(text).unwrap();
        let order = |destination| OrderView {
            status: Some(CardOrderStatus::Delivered),
            paid: Some(Money::from_minor_units(1000, Currency::Usd)),
            cycles_delivered: Some(6_959_866_863_905),
            destination,
            ..order_with(None, None)
        };
        let buyer = principal(BUYER);
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

    const SHOWINGS: [Showing; 4] = [
        Showing::Phase(PurchasePhase::AwaitingPayment),
        Showing::Phase(PurchasePhase::PaymentPending),
        Showing::Phase(PurchasePhase::Delivering),
        Showing::NoAnswer,
    ];

    /// CyclePay's ids are `co-<n>-<32 hex>`, 37 to 41 characters today: the
    /// live line names the order by its number, and fits 80 columns.
    #[test]
    fn the_live_line_names_the_order_by_its_number_and_fits_80_columns() {
        let id = "co-1234-0123456789abcdef0123456789abcdef";
        assert_eq!(order_number(id), "co-1234");
        assert_eq!(order_number("co-6-5e64b574"), "co-6");
        assert_eq!(order_number("o1"), "o1");
        let prefix = format!("[order {}]", order_number(id));
        for showing in SHOWINGS {
            let line = format!("{prefix} ✷ {}", live_message(showing, None, &prefix, 80));
            assert!(
                line.chars().count() <= 80,
                "{} columns: {line}",
                line.chars().count()
            );
            // The widest message fits too, for the longest number the doc
            // promises.
            let widest = live_message(showing, None, "", usize::MAX);
            assert!(format!("[order co-123456] ✷ {widest}").chars().count() <= 80);
        }
    }

    #[test]
    fn timestamps_and_ages_read_well() {
        assert_eq!(format_secs(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_age(Duration::from_secs(90)), "1 minute");
        assert_eq!(format_age(Duration::from_secs(5 * 3600)), "5 hours");
        assert_eq!(format_age(Duration::from_secs(3 * 86_400)), "3 days");
    }

    /// `--cycles` names what was asked for, and the estimate is what the
    /// price buys by the gateway's arithmetic: at the boundary the least
    /// cent's estimate covers the cycles asked for, and the block promises
    /// nothing more than that estimate.
    #[test]
    fn the_quote_block_asks_for_cycles_without_promising_them() {
        use crate::cyclepay::pricing::{least_gross_for, ledger_cycles};
        let gateway = gateway_at(CYCLES_GATEWAY_CID);
        let rate = Rate {
            xdr_usd_x10000: 13_566,
            stale: false,
            age: None,
        };
        // Exactly what $1.00 buys, and one cycle more, which takes a cent
        // more.
        let at_one_dollar = ledger_cycles(100, rate.xdr_usd_x10000, &gateway.terms);
        for (wanted, cents) in [(at_one_dollar, 100), (at_one_dollar + 1, 101)] {
            let target = Target::Receive {
                cycles: wanted,
                currency: Currency::Usd,
            };
            let quote = purchase::quote_at(&gateway, rate, target).unwrap();
            assert_eq!(quote.amount.minor_units, cents);
            assert_eq!(
                least_gross_for(wanted, rate.xdr_usd_x10000, &gateway.terms),
                Some(cents)
            );
            assert!(quote.cycles >= wanted);
            let buyer = Principal::from_text(BUYER).unwrap();
            let block = quote_block(&quote, target, &gateway, buyer, None);
            assert_eq!(block[2], format!("Wanted:   {}", format_cycles(wanted)));
            assert!(
                block.iter().all(|line| !line.contains("at least")),
                "{block:?}"
            );
            assert!(block[4].starts_with("Estimate: about "), "{block:?}");
        }
    }

    /// A prompt that answers as told, and remembers what was done with it.
    struct FakePrompt {
        terminals: (bool, bool),
        answer: Answer,
        /// Press Ctrl-C while the question is up: SIGINT to this process,
        /// which ends it unless something catches the signal.
        ctrl_c: bool,
        asked: bool,
        abandoned: bool,
    }

    impl FakePrompt {
        fn answering(answer: Answer) -> Self {
            Self {
                terminals: (true, true),
                answer,
                ctrl_c: false,
                asked: false,
                abandoned: false,
            }
        }
    }

    impl Prompt for FakePrompt {
        fn terminals(&self) -> (bool, bool) {
            self.terminals
        }

        fn ask(&mut self, _question: &str) -> Result<Answer, dialoguer::Error> {
            self.asked = true;
            #[cfg(unix)]
            if self.ctrl_c {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
            }
            Ok(self.answer)
        }

        fn abandon(&mut self) {
            self.abandoned = true;
        }
    }

    /// Confirm the quote on CyclePay's gateway, without `--yes`, at `prompt`.
    fn confirm_at(prompt: &mut FakePrompt) -> Result<Interrupt, anyhow::Error> {
        let quote = quote();
        confirm(
            &quote,
            Target::Spend(quote.amount),
            &gateway_at(CYCLES_GATEWAY_CID),
            Principal::from_text(BUYER).unwrap(),
            None,
            false,
            true,
            prompt,
        )
    }

    /// The prompt needs a terminal to read the answer on and one to draw the
    /// question on: stdin and stderr. Either redirected (`2>&1 | tee` sends
    /// stderr down a pipe) is a refusal that names `--yes`, not an IO error,
    /// and the question is never asked.
    #[tokio::test]
    async fn the_prompt_needs_stdin_and_stderr_on_a_terminal() {
        assert_eq!(cannot_prompt(true, true), None);
        for terminals in [(true, false), (false, true), (false, false)] {
            let mut prompt = FakePrompt {
                terminals,
                ..FakePrompt::answering(Answer::Yes)
            };
            let err = confirm_at(&mut prompt).err().unwrap().to_string();
            assert_eq!(
                Some(err.clone()),
                cannot_prompt(terminals.0, terminals.1).map(|why| paragraphs(&[why])),
                "{terminals:?}"
            );
            assert!(err.lines().all(|line| line.chars().count() <= 80), "{err}");
            assert!(
                err.replace('\n', " ").contains("Use --yes to proceed"),
                "{err}"
            );
            assert!(!prompt.asked, "{terminals:?}");
        }
        let mut prompt = FakePrompt {
            terminals: (true, false),
            ..FakePrompt::answering(Answer::Yes)
        };
        let err = confirm_at(&mut prompt).err().unwrap().to_string();
        assert!(err.replace('\n', " ").contains("drawn on stderr"), "{err}");
    }

    /// An error that is the whole story is wrapped to 80 columns, the
    /// first line leaving room for `Error: `; one with a cause keeps its
    /// chain, for anyhow to print beneath it.
    #[test]
    fn errors_are_wrapped_and_keep_their_causes() {
        let gateway = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();
        let whole = reported(CyclesPurchaseError::NotAGateway {
            gateway,
            method: "get_card_terms",
        });
        assert_eq!(
            whole.to_string(),
            "canister ryjl3-tyaaa-aaaaa-aaaba-cai is not a cycles gateway: it does not\n\
             serve get_card_terms, the method a gateway publishes its terms with"
        );
        let caused = reported(CyclesPurchaseError::ReadTerms {
            gateway,
            source: Box::new(icp_project::calls::TypedCallError::Call {
                source: Box::new(icp_project::calls::CallError::Rejected {
                    canister: gateway,
                    method: "get_card_terms".to_owned(),
                    code: Some("IC0503".to_owned()),
                    message: "the canister trapped".to_owned(),
                }),
            }),
        });
        assert!(caused.chain().count() > 1, "{caused:?}");
        assert!(
            caused
                .chain()
                .any(|cause| cause.to_string().contains("trapped")),
            "{caused:?}"
        );
    }

    /// Ctrl-C at the question is an answer, not the end of the process: the
    /// signal is caught before the question is asked (were it not, SIGINT
    /// would end this test binary), the prompt gives the cursor back, and no
    /// order is made.
    #[cfg(unix)]
    #[tokio::test]
    async fn ctrl_c_at_the_prompt_is_an_answer() {
        let mut prompt = FakePrompt {
            ctrl_c: true,
            ..FakePrompt::answering(Answer::Interrupted)
        };
        let err = confirm_at(&mut prompt).err().unwrap();
        assert_eq!(err.to_string(), "no order created");
        assert!(prompt.asked && prompt.abandoned);

        let mut prompt = FakePrompt::answering(Answer::No);
        let err = confirm_at(&mut prompt).err().unwrap();
        assert_eq!(err.to_string(), "no order created");
        assert!(!prompt.abandoned);

        let mut prompt = FakePrompt::answering(Answer::Yes);
        assert!(confirm_at(&mut prompt).is_ok());
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

    /// A refund under way is not an end: the command says it can still end
    /// owed a refund, and how to read the order back to see which, as the
    /// identity that made it.
    #[test]
    fn a_refund_under_way_says_how_to_see_how_it_ends() {
        let order = OrderView {
            detail: Some("DELIVERY_FAILED".into()),
            ..order_with(Some(CardOrderStatus::Refunding), None)
        };
        let names = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        let err = ending(&order, names, &after()).unwrap_err().to_string();
        assert_eq!(
            err,
            "\
order co-1 could not be delivered (DELIVERY_FAILED).
A refund to your card is under way. It ends refunded, or owed a refund that
CyclePay makes by hand. To see which:

    icp canister call 2bwus-iiaaa-aaaac-bee3a-cai order_status --query --identity alice -n ic '(\"co-1\")'"
        );
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
        for status in [CardOrderStatus::RefundOwed, CardOrderStatus::NeedsReview] {
            let err = ending(&order_with(Some(status), None), names, &after())
                .unwrap_err()
                .to_string();
            assert!(!err.contains("CyclePay"), "{err}");
        }
        assert!(!not_known(&quote(), names, &after().flags).contains("CyclePay"));
        let cyclepay = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        assert_eq!(cyclepay.page, "Stripe's page");
        assert!(
            stale_rate_warning(&stale, cyclepay)
                .unwrap()
                .starts_with("CyclePay has not refreshed")
        );
    }

    /// The live line picks the longest message that fits, so that none is
    /// cut: at 60 columns the Ctrl-C hint is shorter, not truncated into
    /// "leaves the order", and at 44 it is dropped.
    #[test]
    fn the_live_line_fits_the_terminal_without_cutting_its_hint() {
        let prefix = "[order co-12]";
        let awaiting = Showing::Phase(PurchasePhase::AwaitingPayment);
        assert_eq!(
            live_message(awaiting, None, prefix, 80),
            "Waiting for payment; Ctrl-C stops waiting, not the order"
        );
        assert_eq!(
            live_message(awaiting, None, prefix, 60),
            "Waiting for payment; Ctrl-C to stop"
        );
        assert_eq!(
            live_message(awaiting, None, prefix, 44),
            "Waiting for payment"
        );
        assert_eq!(
            live_message(awaiting, None, prefix, 10),
            "Waiting for payment"
        );
        for showing in SHOWINGS {
            for columns in [30, 44, 60, 80, 120] {
                let message = live_message(showing, None, prefix, columns);
                let line = format!("{prefix} ✷ {message}");
                assert!(
                    line.chars().count() <= columns || message == shortest(showing),
                    "{columns}: {line}"
                );
            }
        }
        // A payment that has not settled is not called received.
        let pending = Showing::Phase(PurchasePhase::PaymentPending);
        assert!(!live_message(pending, None, prefix, 80).contains("received"));
        assert!(!line_message(pending, None).contains("received"));
    }

    /// The message fills the row to its last column, which indicatif pads
    /// to anyway, and not one column more: at exactly the width of a
    /// message, that message; one column less, the next one down.
    #[test]
    fn the_live_line_may_reach_the_last_column_but_not_wrap() {
        let prefix = "[order co-12]";
        for showing in SHOWINGS {
            let mut choices = Vec::new();
            for columns in (0..=120).rev() {
                let message = live_message(showing, None, prefix, columns);
                if choices.last() != Some(&message) {
                    choices.push(message);
                }
            }
            for pair in choices.windows(2) {
                let [longer, shorter] = pair else {
                    unreachable!()
                };
                let width = format!("{prefix} ✷ {longer}").chars().count();
                assert_eq!(live_message(showing, None, prefix, width), *longer);
                assert_eq!(live_message(showing, None, prefix, width - 1), *shorter);
            }
        }
    }

    /// The shortest message of a state, which is shown however narrow the
    /// terminal.
    fn shortest(showing: Showing) -> String {
        live_message(showing, None, "", 0)
    }

    /// A delivery that waits on something says what, as the gateway puts
    /// it: whole off a terminal, and on the live line cut with `…` to fit,
    /// never into the phase's own words.
    #[test]
    fn the_gateways_note_is_shown_cut_to_fit_on_the_live_line() {
        let prefix = "[order co-12]";
        let delivering = Showing::Phase(PurchasePhase::Delivering);
        let note = "CMC rate stale: 90000s old (max allowed 86400s) — RATE_STALE";
        assert_eq!(
            line_message(delivering, Some(note)),
            format!("Payment received; delivery waits: {note}")
        );
        assert_eq!(
            live_message(delivering, Some(note), prefix, 120),
            format!("Delivery waits: {note}")
        );
        let cut = live_message(delivering, Some(note), prefix, 60);
        assert_eq!(cut, "Delivery waits: CMC rate stale: 90000s old…");
        assert!(format!("{prefix} ✷ {cut}").chars().count() <= 60);
        // Too narrow for any of the note: the phase alone.
        assert_eq!(
            live_message(delivering, Some(note), prefix, 34),
            "Delivering cycles..."
        );
        // A note keeps to its row.
        assert_eq!(on_one_row("a\nb\r\tc"), "a b  c");
    }

    /// The flags a run was given come back for the commands that check on
    /// the order, the default identity by its name, quoted where a shell
    /// needs it, and a name no quoting serves left for the reader to fill
    /// in, and said to be.
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
                "Fill in <your identity> yourself: no quoting of it works in every shell."
            ),
            "{err}"
        );
    }

    /// Stopped before the gateway answered whether it made the order, the
    /// command says it may or may not have, and, on CyclePay, how to find
    /// out: the same amount asked for again within the hour hands the open
    /// order back.
    #[test]
    fn stopped_while_ordering_says_the_order_may_exist() {
        let names = Names::of(&gateway_at(CYCLES_GATEWAY_CID));
        let said = not_known(&quote(), names, &after().flags);
        assert_eq!(
            said,
            "\
stopped before CyclePay answered; the order may or may not exist.
Nothing is charged unless it is paid. To find out, run the command again within
the hour with the same amount: CyclePay hands back the open order if it made
one, rather than making another.

    icp cycles buy --amount 10.00 --identity alice -n ic"
        );
        assert_laid_out(&said);
        let other = Names::of(&gateway_at("ryjl3-tyaaa-aaaaa-aaaba-cai"));
        let said = not_known(&quote(), other, &after().flags);
        assert!(said.contains("may or may not exist"), "{said}");
        assert!(!said.contains("icp cycles buy"), "{said}");
        assert_laid_out(&said);
    }
}
