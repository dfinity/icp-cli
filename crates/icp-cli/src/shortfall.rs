//! Running short of cycles on the cycles ledger: how many are missing, and
//! the commands that make them up.
//!
//! Reached only from the ledger's refusal for want of cycles (creating a
//! canister, or topping one up), and built from that refusal's figures, so a
//! command that has enough never says any of it, and nothing here reads a
//! balance of its own. On mainnet it asks the card gateway what one order
//! buys, so as never to print an `icp cycles buy` that would be refused.
//! On a network `icp network start` runs, which funds its anonymous identity
//! and only the identities there when it started, it offers a transfer from
//! the anonymous identity.

use std::time::Duration;

use candid::Principal;
use ic_agent::Agent;
use icp_app::context::{Context, NetworkSelection};
use icp_app::identity::IdentitySelection;
use icp_project::calls::{CallError, CanisterCalls, TypedCallError};
use icp_project::host::EnvironmentSelection;
use icp_project::network::Configuration;
use icp_project::parsers::display_cycles;

use crate::commands::args::{PasteFlags, selection_flags, shell_word};
use crate::cyclepay::interface::cycles_gateway_canister_id;
use crate::cyclepay::money::Currency;
use crate::cyclepay::purchase::{self, CyclesPurchaseError, Gateway, Rate, Target};

/// How long the card gateway may take to say what one order buys before the
/// hint goes without it.
const CARD_TERMS_TIMEOUT: Duration = Duration::from_secs(5);

/// What to say about a refusal for want of cycles.
pub(crate) struct Hint {
    /// Everything the refused calls needed, the ledger's fees included.
    required: u128,
    /// The identity's balance, as the ledger reported it when it refused.
    balance: u128,
    /// The identity, as the message names it: `identity alice`.
    who: String,
    remedy: Remedy,
}

/// Where the missing cycles can come from.
enum Remedy {
    /// Nowhere, for the anonymous identity on mainnet: anyone can spend what
    /// it holds. `chosen` when `--identity` asked for it, rather than its
    /// being the default.
    OwnIdentity { chosen: bool },
    /// `icp cycles mint`, asking for `cycles` as the identity and on the
    /// network the refused command ran with, and `also` another way.
    Get {
        cycles: u128,
        flags: PasteFlags,
        also: Also,
    },
}

/// Another way to the missing cycles than converting ICP.
enum Also {
    /// None.
    Nothing,
    /// A transfer to `to` from the anonymous identity, which a local network
    /// funds, selected by `flags`.
    FromAnonymous { to: Principal, flags: PasteFlags },
    /// A card order, as far as one goes.
    Cards(Cards),
}

/// Where a refused command ran, as far as the hint goes.
enum Place {
    /// Mainnet, where a card order can do what [`Cards`] says.
    Mainnet(Cards),
    /// A network `icp network start` runs, which funds its anonymous
    /// identity, as `caller`.
    Local { caller: Principal },
    /// Any other network.
    Elsewhere,
}

/// What one card order (`icp cycles buy`) can do for a shortfall.
#[derive(Debug, PartialEq)]
enum Cards {
    /// It buys it all.
    Cover,
    /// It buys no more than `most`.
    Short { most: u128 },
    /// The gateway gave no answer: it may well next time.
    Unknown,
    /// The gateway answered that it takes no order now, or is no gateway.
    Unavailable,
}

/// The cycles one canister needs at the least: the IC's creation fee on a
/// 13-node subnet, and the cycles ledger's fee.
const LEAST_PER_CANISTER: u128 = 500_000_000_000 + 100_000_000;

impl Hint {
    /// The hint for a refusal of work needing `required` cycles (fees
    /// included) from a `balance` of them, as the caller `agent` signs for,
    /// run as `identity` on `network` or `environment`.
    ///
    /// On mainnet, told apart by the network's root key as `icp cycles buy`
    /// tells it, the gateway that command would use is asked whether one
    /// order covers the shortfall, as that command would check it. On a
    /// network `icp network start` runs, the anonymous identity it funds can
    /// make it up.
    ///
    /// `None` when the figures say nothing is missing, as a balance too large
    /// to read would, or when the caller is unknown: no hint rather than a
    /// wrong one.
    pub(crate) async fn read(
        ctx: &Context,
        agent: &Agent,
        required: u128,
        balance: u128,
        identity: &IdentitySelection,
        network: &NetworkSelection,
        environment: &EnvironmentSelection,
    ) -> Option<Self> {
        if balance >= required {
            return None;
        }
        let caller = agent.get_principal().ok()?;
        let anonymous = caller == Principal::anonymous();
        let place = if purchase::is_mainnet(&agent.read_root_key()) {
            match anonymous {
                true => Place::Mainnet(Cards::Unavailable),
                false => Place::Mainnet(cards(agent, ask(required - balance)).await),
            }
        } else if managed(ctx, network, environment).await {
            Place::Local { caller }
        } else {
            Place::Elsewhere
        };
        Self::new(
            required,
            balance,
            anonymous,
            place,
            identity,
            network,
            environment,
        )
    }

    /// [`Self::read`] once the caller and the place are known.
    fn new(
        required: u128,
        balance: u128,
        anonymous: bool,
        place: Place,
        identity: &IdentitySelection,
        network: &NetworkSelection,
        environment: &EnvironmentSelection,
    ) -> Option<Self> {
        if balance >= required {
            return None;
        }
        let who = match identity {
            _ if anonymous => "the anonymous identity".to_owned(),
            // As a command would spell it, so that it reads as one name.
            IdentitySelection::Named(name) => shell_word(name).map_or_else(
                || "the identity".to_owned(),
                |name| format!("identity {name}"),
            ),
            IdentitySelection::Default | IdentitySelection::Anonymous => "the identity".to_owned(),
        };
        let also = match place {
            Place::Mainnet(_) if anonymous => {
                let chosen = !matches!(identity, IdentitySelection::Default);
                return Some(Self {
                    required,
                    balance,
                    who,
                    remedy: Remedy::OwnIdentity { chosen },
                });
            }
            Place::Mainnet(cards) => Also::Cards(cards),
            // Not from the anonymous identity to itself.
            Place::Local { caller } if !anonymous => Also::FromAnonymous {
                to: caller,
                flags: selection_flags(&IdentitySelection::Anonymous, network, environment),
            },
            Place::Local { .. } | Place::Elsewhere => Also::Nothing,
        };
        Some(Self {
            required,
            balance,
            who,
            remedy: Remedy::Get {
                cycles: ask(required - balance),
                flags: selection_flags(identity, network, environment),
                also,
            },
        })
    }

    /// The error a command reports: what it could not do (`create 2
    /// canisters`, `top up backend with 5T cycles`), what that takes against
    /// the balance, and how to make up the difference before doing `again`
    /// (`run the deploy again`). `fewer`, for a creation, names the
    /// canisters (`each canister`) and counts them, for a last sentence on
    /// giving them fewer cycles, said where the balance could cover that.
    /// Prose is wrapped to 80 columns, `Error: ` included; the commands
    /// stand on lines of their own.
    pub(crate) fn message(&self, doing: &str, again: &str, fewer: Option<(&str, usize)>) -> String {
        let mut message = Message::default();
        message.prose(&format!("not enough cycles to {doing}."));
        let takes = format!("It takes {}, fees included,", cycles(self.required));
        let has = format!("and {} has {}.", self.who, display_cycles(self.balance));
        match format!("{takes} {has}") {
            line if line.chars().count() <= WIDTH => message.prose(&line),
            _ => {
                message.prose(&takes);
                message.prose(&has);
            }
        }
        match &self.remedy {
            Remedy::OwnIdentity { chosen } => {
                message.prose(
                    "Cycles need an identity of your own: anyone can spend the anonymous one's.",
                );
                if *chosen {
                    message.prose(&format!("Make one, then {again} with --identity <name>:"));
                    message.commands(&["icp identity new <name>".to_owned()]);
                } else {
                    message.prose(&format!("Make one and make it the default, then {again}:"));
                    message.commands(&[
                        "icp identity new <name>".to_owned(),
                        "icp identity default <name>".to_owned(),
                    ]);
                }
                return message.text();
            }
            Remedy::Get {
                cycles: ask,
                flags,
                also,
            } => {
                let ask = display_cycles(*ask);
                let mint = format!("icp cycles mint --cycles {ask}{}", flags.text);
                let mut notes = flags.notes();
                match also {
                    Also::FromAnonymous { to, flags } => {
                        message.prose(&format!(
                            "Get {ask} more, from the anonymous identity, which a local network \
                             funds, or from ICP, then {again}:"
                        ));
                        message.commands(&[
                            format!("icp cycles transfer {ask} {to}{}", flags.text),
                            mint,
                        ]);
                        notes.extend(flags.notes());
                    }
                    Also::Cards(Cards::Cover) => {
                        message.prose(&format!(
                            "Get {ask} more, with a card or from ICP, then {again}:"
                        ));
                        message.commands(&[
                            format!("icp cycles buy --cycles {ask}{}", flags.text),
                            mint,
                        ]);
                    }
                    _ => {
                        message.prose(&format!("Get {ask} more from ICP, then {again}:"));
                        message.commands(&[mint]);
                    }
                }
                match also {
                    Also::Cards(Cards::Short { most }) => message.prose(&format!(
                        "Card orders (icp cycles buy) buy up to about {} each.",
                        cycles(three_figures(*most, false))
                    )),
                    Also::Cards(Cards::Unknown) => message
                        .prose("They can be bought with a card too: see icp cycles buy --help."),
                    _ => {}
                }
                notes.dedup();
                for note in notes {
                    message.prose(&note);
                }
            }
        }
        if let Some((canisters, count)) = fewer
            && self.balance >= LEAST_PER_CANISTER.saturating_mul(count as u128)
        {
            message.prose(&format!(
                "You can also give {canisters} fewer cycles with --cycles,"
            ));
            message.prose("at least the 0.5T a creation costs.");
        }
        message.text()
    }
}

/// Whether the refused command ran on a network `icp network start` runs.
async fn managed(
    ctx: &Context,
    network: &NetworkSelection,
    environment: &EnvironmentSelection,
) -> bool {
    let network = match network {
        NetworkSelection::Default => ctx
            .host
            .get_environment(environment)
            .await
            .map(|environment| environment.network)
            .ok(),
        network => ctx.get_network(network).await.ok(),
    };
    network.is_some_and(|network| matches!(network.configuration, Configuration::Managed { .. }))
}

/// How wide a line of the message may be.
const WIDTH: usize = 80;

/// A message built up of prose and of commands set apart.
#[derive(Default)]
pub(crate) struct Message {
    lines: Vec<String>,
}

impl Message {
    /// `text`, wrapped at spaces to [`WIDTH`]; the first line of the message
    /// leaves room for `Error: `.
    pub(crate) fn prose(&mut self, text: &str) {
        let mut line = String::new();
        for word in text.split(' ') {
            let room = if self.lines.is_empty() {
                WIDTH - "Error: ".len()
            } else {
                WIDTH
            };
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > room {
                self.lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        self.lines.push(line);
    }

    /// `commands`, indented, each on a line of its own and never wrapped,
    /// with a blank line either side.
    pub(crate) fn commands(&mut self, commands: &[String]) {
        self.lines.push(String::new());
        self.lines
            .extend(commands.iter().map(|command| format!("    {command}")));
        self.lines.push(String::new());
    }

    /// The lines, with nothing after the last.
    pub(crate) fn text(mut self) -> String {
        while self.lines.last().is_some_and(String::is_empty) {
            self.lines.pop();
        }
        self.lines.join("\n")
    }
}

/// How far one card order would go for `cycles`, asked of the gateway `icp
/// cycles buy` would use.
async fn cards(agent: &Agent, cycles: u128) -> Cards {
    // A gateway named wrongly in the environment is none to offer.
    let Ok(gateway) = cycles_gateway_canister_id() else {
        return Cards::Unavailable;
    };
    match icp_app::calls::calls(agent.clone(), None) {
        Ok(calls) => ask_gateway(calls.as_ref(), gateway, cycles).await,
        Err(_) => Cards::Unknown,
    }
}

/// [`cards`], asked of `canister` through `calls`, waiting no longer than
/// [`CARD_TERMS_TIMEOUT`] for its terms and rate.
async fn ask_gateway(calls: &dyn CanisterCalls, canister: Principal, cycles: u128) -> Cards {
    let answer = async {
        let gateway = purchase::connect(calls, canister).await?;
        let rate = purchase::read_rate(calls, &gateway).await?;
        Ok((gateway, rate))
    };
    match tokio::time::timeout(CARD_TERMS_TIMEOUT, answer).await {
        Ok(answer) => card_order(answer, cycles),
        Err(_elapsed) => Cards::Unknown,
    }
}

/// The share of an order kept free for the rate to move before the buy:
/// one part in this many.
const SLACK: u128 = 50;

/// What one card order does for `cycles`, from the gateway's answer: its
/// terms and rate, checked as `icp cycles buy` checks them for a little more
/// (see [`SLACK`]), or the error reading them.
fn card_order(answer: Result<(Gateway, Rate), CyclesPurchaseError>, cycles: u128) -> Cards {
    let (gateway, rate) = match answer {
        Ok(answer) => answer,
        Err(error) if no_verdict(&error) => return Cards::Unknown,
        // An answer, but none a card order can be placed on: a canister that
        // is not a gateway, or terms this CLI cannot act on.
        Err(_) => return Cards::Unavailable,
    };
    let target = Target::Receive {
        cycles: cycles.saturating_add(cycles / SLACK),
        currency: Currency::Usd,
    };
    match purchase::quote_at(&gateway, rate, target) {
        Ok(_) => Cards::Cover,
        // Less the slack, which is what a hint can ask of one order.
        Err(CyclesPurchaseError::CyclesAboveMaximum { buys, .. }) => Cards::Short {
            most: buys.saturating_mul(SLACK) / (SLACK + 1),
        },
        // Paused.
        Err(_) => Cards::Unavailable,
    }
}

/// Whether reading the gateway reached no verdict: the call went
/// unanswered, or the rate is not set yet. Another try may well do.
fn no_verdict(error: &CyclesPurchaseError) -> bool {
    match error {
        CyclesPurchaseError::ReadTerms { source, .. }
        | CyclesPurchaseError::ReadRate { source, .. } => matches!(
            &**source,
            TypedCallError::Call { source } if matches!(**source, CallError::Unanswered { .. })
        ),
        CyclesPurchaseError::RateUnavailable { .. } => true,
        _ => false,
    }
}

/// `2.01T cycles`, `1 cycle`.
pub(crate) fn cycles(cycles: u128) -> String {
    let plural = if cycles == 1 { "" } else { "s" };
    format!("{} cycle{plural}", display_cycles(cycles))
}

/// What to ask for when `missing` cycles are missing: 0.5% and 1M cycles
/// more, so that neither the exchange rate moving before they arrive nor
/// ICP's rounding to e8s (some 22k cycles) leaves them short, rounded up to
/// three significant figures to read plainly.
fn ask(missing: u128) -> u128 {
    const MARGIN: u128 = 1_000_000;
    three_figures(
        missing.saturating_add(missing / 200).saturating_add(MARGIN),
        true,
    )
}

/// `cycles` to three significant figures, rounded `up` or down.
fn three_figures(cycles: u128, up: bool) -> u128 {
    let digits = cycles.checked_ilog10().map_or(0, |log| log + 1);
    if digits <= 3 {
        return cycles;
    }
    let unit = 10u128.pow(digits - 3);
    if up {
        cycles.div_ceil(unit).saturating_mul(unit)
    } else {
        cycles / unit * unit
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use async_trait::async_trait;
    use candid::{Encode, Nat};
    use clap::Parser;
    use icp_project::calls::{Authority, Call};
    use icp_project::network::RootKeySpec;

    use super::*;
    use crate::cyclepay::interface::{CARD_TERMS_METHOD, CardTerms, XdrRateStatus};
    use crate::cyclepay::pricing::ledger_cycles;
    use crate::{Cli, Command, commands::cycles};

    const T: u128 = 1_000_000_000_000;
    const B: u128 = 1_000_000_000;
    /// The cycles ledger's fee on a creation or a withdrawal.
    const FEE: u128 = 100_000_000;
    const EACH: Option<(&str, usize)> = Some(("each canister", 1));
    const DEPLOY: &str = "run the deploy again";
    const CREATE: &str = "create the canister again";
    /// CyclePay's backend on mainnet.
    const GATEWAY: &str = "2bwus-iiaaa-aaaac-bee3a-cai";
    /// An identity made after its local network started.
    const LATE: &str = "nmxlr-f5o22-e7rm6-juz7x-ewgfs-b35mw-7ed5p-n6sff-we3wi-ioapo-nae";
    /// $1.3520 an XDR.
    const RATE: u64 = 13_520;

    fn named(identity: &str) -> IdentitySelection {
        IdentitySelection::Named(identity.into())
    }

    fn env(name: &str) -> EnvironmentSelection {
        EnvironmentSelection::Named(name.into())
    }

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    /// A hint as `alice` on environment `ic`, on mainnet where `cards` say.
    fn as_alice(required: u128, balance: u128, cards: Cards) -> Option<Hint> {
        Hint::new(
            required,
            balance,
            false,
            Place::Mainnet(cards),
            &named("alice"),
            &NetworkSelection::Default,
            &env("ic"),
        )
    }

    #[test]
    fn the_shortfall_is_asked_for_with_a_margin_in_three_figures() {
        // One canister at the default 2T, the ledger's fee on top, from 500B.
        assert_eq!(ask(2 * T + FEE - 500 * B), 1_510_000_000_000);
        // One or three canisters from nothing.
        assert_eq!(ask(2 * T + FEE), 2_020_000_000_000);
        assert_eq!(ask(3 * (2 * T + FEE)), 6_040_000_000_000);
        // Already round: still more.
        assert_eq!(ask(4 * T), 4_030_000_000_000);
        // Small: the margin, at least.
        assert_eq!(ask(1), 1_010_000);
        assert_eq!(ask(FEE), 102_000_000);
        // Never less than what is missing and the margin, and never a panic.
        for missing in [1, 999, 1_000, 1_234_567, 2 * T + FEE, 200 * T, u128::MAX] {
            assert!(
                ask(missing) >= missing.saturating_add(missing / 200 + 1_000_000),
                "{missing}"
            );
        }
        assert_eq!(three_figures(142_934_000_000_000, false), 142 * T);
        assert_eq!(three_figures(999, false), 999);
    }

    /// From nothing, on mainnet, where one card order covers it: bought with
    /// a card, or converted from ICP, as the identity and on the environment
    /// given. Fewer cycles would not do from nothing, so that goes unsaid.
    #[test]
    fn from_a_zero_balance_on_mainnet() {
        let hint = as_alice(2 * T + FEE, 0, Cards::Cover).unwrap();
        assert_eq!(
            hint.message("create 1 canister", DEPLOY, EACH),
            "not enough cycles to create 1 canister.\n\
             It takes 2.0001T cycles, fees included, and identity alice has 0.\n\
             Get 2.02T more, with a card or from ICP, then run the deploy again:\n\n    \
             icp cycles buy --cycles 2.02T --identity alice -e ic\n    \
             icp cycles mint --cycles 2.02T --identity alice -e ic"
        );
    }

    /// More than one card order buys: converting ICP alone, and what one
    /// order buys.
    #[test]
    fn past_what_one_card_order_buys() {
        let hint = as_alice(
            200 * T + FEE,
            0,
            Cards::Short {
                most: 140_131_000_000_000,
            },
        )
        .unwrap();
        assert_eq!(
            hint.message("create the canister", CREATE, None),
            "not enough cycles to create the canister.\n\
             It takes 200.0001T cycles, fees included, and identity alice has 0.\n\
             Get 202T more from ICP, then create the canister again:\n\n    \
             icp cycles mint --cycles 202T --identity alice -e ic\n\n\
             Card orders (icp cycles buy) buy up to about 140T cycles each."
        );
    }

    /// On mainnet with the card gateway silent: converting ICP alone, and
    /// cards named with no figure. With a gateway that answered it takes no
    /// order, nothing about cards.
    #[test]
    fn when_the_card_gateway_cannot_say() {
        let hint = as_alice(2 * T + FEE, 0, Cards::Unknown).unwrap();
        assert_eq!(
            hint.message("create the canister", CREATE, None),
            "not enough cycles to create the canister.\n\
             It takes 2.0001T cycles, fees included, and identity alice has 0.\n\
             Get 2.02T more from ICP, then create the canister again:\n\n    \
             icp cycles mint --cycles 2.02T --identity alice -e ic\n\n\
             They can be bought with a card too: see icp cycles buy --help."
        );
        let hint = as_alice(2 * T + FEE, 0, Cards::Unavailable).unwrap();
        assert_eq!(
            hint.message("create the canister", CREATE, None),
            "not enough cycles to create the canister.\n\
             It takes 2.0001T cycles, fees included, and identity alice has 0.\n\
             Get 2.02T more from ICP, then create the canister again:\n\n    \
             icp cycles mint --cycles 2.02T --identity alice -e ic"
        );
    }

    /// On a network that is not mainnet and that `icp network start` does not
    /// run, only converting ICP, and no word of cards. With enough for a
    /// cheaper canister, fewer cycles is said, and the least.
    #[test]
    fn elsewhere_only_converting_icp() {
        let hint = Hint::new(
            2 * T + FEE,
            T,
            false,
            Place::Elsewhere,
            &IdentitySelection::Default,
            &NetworkSelection::Default,
            &env("staging"),
        )
        .unwrap();
        let said = hint.message("create 1 canister", DEPLOY, EACH);
        assert_eq!(
            said,
            "not enough cycles to create 1 canister.\n\
             It takes 2.0001T cycles, fees included, and the identity has 1T.\n\
             Get 1.01T more from ICP, then run the deploy again:\n\n    \
             icp cycles mint --cycles 1.01T -e staging\n\n\
             You can also give each canister fewer cycles with --cycles,\n\
             at least the 0.5T a creation costs."
        );
        assert!(!said.contains("card") && !said.contains("buy"), "{said}");
    }

    /// On a local network, an identity made after it started has nothing:
    /// the anonymous identity, which the network funds, transfers to it, or
    /// ICP converts.
    #[test]
    fn on_a_local_network_from_the_anonymous_identity() {
        let hint = Hint::new(
            2 * T + FEE,
            0,
            false,
            Place::Local {
                caller: principal(LATE),
            },
            &named("late"),
            &NetworkSelection::Default,
            &env("random-environment"),
        )
        .unwrap();
        assert_eq!(
            hint.message("create the canister", CREATE, Some(("the canister", 1))),
            format!(
                "not enough cycles to create the canister.\n\
                 It takes 2.0001T cycles, fees included, and identity late has 0.\n\
                 Get 2.02T more, from the anonymous identity, which a local network funds, or\n\
                 from ICP, then create the canister again:\n\n    \
                 icp cycles transfer 2.02T {LATE} --identity anonymous -e random-environment\n    \
                 icp cycles mint --cycles 2.02T --identity late -e random-environment"
            )
        );
    }

    /// The anonymous identity off mainnet gets the ordinary hint: on a local
    /// network nothing to transfer from itself, and converting ICP, as it
    /// was selected.
    #[test]
    fn off_mainnet_the_anonymous_identity_converts_icp() {
        for (identity, flag) in [
            (IdentitySelection::Default, ""),
            (IdentitySelection::Anonymous, " --identity anonymous"),
        ] {
            for place in [
                Place::Local {
                    caller: Principal::anonymous(),
                },
                Place::Elsewhere,
            ] {
                let hint = Hint::new(
                    2 * T + FEE,
                    0,
                    true,
                    place,
                    &identity,
                    &NetworkSelection::Default,
                    &env("local"),
                )
                .unwrap();
                assert_eq!(
                    hint.message("create the canister", CREATE, None),
                    format!(
                        "not enough cycles to create the canister.\n\
                         It takes 2.0001T cycles, fees included, and the anonymous identity has \
                         0.\n\
                         Get 2.02T more from ICP, then create the canister again:\n\n    \
                         icp cycles mint --cycles 2.02T{flag} -e local"
                    )
                );
            }
        }
    }

    /// The anonymous identity on mainnet is offered no command that would
    /// fund it, but the ones that make an identity of one's own, and nothing
    /// about fewer cycles.
    #[test]
    fn on_mainnet_the_anonymous_identity_is_told_to_make_its_own() {
        for cards in [Cards::Cover, Cards::Unavailable] {
            let hint = Hint::new(
                2 * T + FEE,
                T,
                true,
                Place::Mainnet(cards),
                &IdentitySelection::Default,
                &NetworkSelection::Named("ic".into()),
                &EnvironmentSelection::Default,
            )
            .unwrap();
            assert_eq!(
                hint.message("create the canister", CREATE, Some(("the canister", 1))),
                "not enough cycles to create the canister.\n\
                 It takes 2.0001T cycles, fees included, and the anonymous identity has 1T.\n\
                 Cycles need an identity of your own: anyone can spend the anonymous one's.\n\
                 Make one and make it the default, then create the canister again:\n\n    \
                 icp identity new <name>\n    \
                 icp identity default <name>"
            );
        }
        let hint = Hint::new(
            2 * T + FEE,
            1,
            true,
            Place::Mainnet(Cards::Cover),
            &IdentitySelection::Anonymous,
            &NetworkSelection::Default,
            &env("ic"),
        )
        .unwrap();
        assert_eq!(
            hint.message("create 1 canister", DEPLOY, EACH),
            "not enough cycles to create 1 canister.\n\
             It takes 2.0001T cycles, fees included, and the anonymous identity has 1.\n\
             Cycles need an identity of your own: anyone can spend the anonymous one's.\n\
             Make one, then run the deploy again with --identity <name>:\n\n    \
             icp identity new <name>"
        );
    }

    /// One cycle short: every figure exact, the margin asked for, and fewer
    /// cycles said, as the balance would cover a cheaper canister.
    #[test]
    fn from_a_balance_just_below_the_need() {
        let hint = as_alice(2 * T + FEE, 2 * T + FEE - 1, Cards::Cover).unwrap();
        assert_eq!(
            hint.message("create 1 canister", DEPLOY, EACH),
            "not enough cycles to create 1 canister.\n\
             It takes 2.0001T cycles, fees included, and identity alice has 2.000099999999T.\n\
             Get 1.01M more, with a card or from ICP, then run the deploy again:\n\n    \
             icp cycles buy --cycles 1.01M --identity alice -e ic\n    \
             icp cycles mint --cycles 1.01M --identity alice -e ic\n\n\
             You can also give each canister fewer cycles with --cycles,\n\
             at least the 0.5T a creation costs."
        );
    }

    /// Fewer cycles is said only when the balance covers the least each of
    /// the canisters could be given.
    #[test]
    fn fewer_cycles_only_where_they_could_do() {
        let least = 500 * B + FEE;
        let fewer = |balance, canisters| {
            Hint::new(
                canisters * (2 * T + FEE),
                balance,
                false,
                Place::Elsewhere,
                &IdentitySelection::Default,
                &NetworkSelection::Default,
                &env("local"),
            )
            .unwrap()
            .message(
                "create some canisters",
                DEPLOY,
                Some(("each canister", canisters as usize)),
            )
            .contains("fewer cycles")
        };
        assert!(!fewer(0, 1));
        assert!(!fewer(least - 1, 1));
        assert!(fewer(least, 1));
        assert!(!fewer(2 * least - 1, 2));
        assert!(fewer(2 * least, 2));
    }

    /// A value no quoting pastes into every shell is left for the user to
    /// fill in, and said so; the identity goes unnamed.
    #[test]
    fn an_unquotable_identity_is_left_to_fill_in() {
        let hint = Hint::new(
            2 * T + FEE,
            0,
            false,
            Place::Mainnet(Cards::Cover),
            &named("o'neil $x"),
            &NetworkSelection::Default,
            &env("ic"),
        )
        .unwrap();
        assert_eq!(
            hint.message("create the canister", CREATE, None),
            "not enough cycles to create the canister.\n\
             It takes 2.0001T cycles, fees included, and the identity has 0.\n\
             Get 2.02T more, with a card or from ICP, then create the canister again:\n\n    \
             icp cycles buy --cycles 2.02T --identity <your identity> -e ic\n    \
             icp cycles mint --cycles 2.02T --identity <your identity> -e ic\n\n\
             Fill in <your identity> yourself: no quoting of it works in every shell."
        );
    }

    /// A name that needs quoting is named as a command would spell it.
    #[test]
    fn a_quoted_identity_is_named_as_quoted() {
        let hint = Hint::new(
            2 * T + FEE,
            0,
            false,
            Place::Elsewhere,
            &named("my id"),
            &NetworkSelection::Default,
            &env("local"),
        )
        .unwrap();
        assert!(
            hint.message("create the canister", CREATE, None).contains(
                "\nIt takes 2.0001T cycles, fees included, and identity 'my id' has 0.\n"
            ),
        );
    }

    /// A count of cycles in prose: one in the singular, as a top-up of one
    /// cycle names it.
    #[test]
    fn one_cycle_is_singular() {
        assert_eq!(cycles(1), "1 cycle");
        assert_eq!(cycles(0), "0 cycles");
        assert_eq!(cycles(2), "2 cycles");
        assert_eq!(cycles(2 * T + FEE), "2.0001T cycles");
    }

    /// A balance that covers the need, or one too large to have been read,
    /// says nothing is missing: no hint rather than a wrong one.
    #[test]
    fn no_hint_when_nothing_is_missing() {
        assert!(as_alice(2 * T + FEE, 2 * T + FEE, Cards::Cover).is_none());
        assert!(as_alice(2 * T + FEE, u128::MAX, Cards::Cover).is_none());
    }

    /// Prose too long for 80 columns, `Error: ` included, wraps: the figures
    /// sentence at its comma, anything else at its last space that fits.
    #[test]
    fn long_prose_wraps_at_80_columns() {
        let hint = Hint::new(
            10 * T + FEE,
            0,
            false,
            Place::Elsewhere,
            &named("financially-challenged"),
            &NetworkSelection::Default,
            &env("random-environment"),
        )
        .unwrap();
        assert_eq!(
            hint.message(
                "top up a-canister-whose-name-runs-on-for-quite-a-while with 10T cycles",
                "run the top-up again",
                None
            ),
            "not enough cycles to top up\n\
             a-canister-whose-name-runs-on-for-quite-a-while with 10T cycles.\n\
             It takes 10.0001T cycles, fees included,\n\
             and identity financially-challenged has 0.\n\
             Get 10.1T more from ICP, then run the top-up again:\n\n    \
             icp cycles mint --cycles 10.1T --identity financially-challenged -e random-environment"
        );
    }

    /// Every line of prose fits 80 columns, `Error: ` included, for figures
    /// and names as wide as they come; only the commands, never wrapped, may
    /// run longer.
    #[test]
    fn the_message_fits_80_columns() {
        let wide = 1_234_567_890_123;
        let long = named("an-identity-with-a-name-long-enough-to-crowd-a-line");
        let places = || {
            [
                Place::Mainnet(Cards::Cover),
                Place::Mainnet(Cards::Short { most: wide }),
                Place::Mainnet(Cards::Unknown),
                Place::Mainnet(Cards::Unavailable),
                Place::Local {
                    caller: principal(LATE),
                },
                Place::Elsewhere,
            ]
        };
        for anonymous in [false, true] {
            for place in places() {
                let hint = Hint::new(
                    12_345 * T + 10 * FEE,
                    wide,
                    anonymous,
                    place,
                    &long,
                    &NetworkSelection::Default,
                    &env("ic"),
                )
                .unwrap();
                let said = hint.message("create 10 canisters", DEPLOY, Some(("each canister", 2)));
                for line in format!("Error: {said}").lines() {
                    assert!(
                        line.starts_with("    icp ") || line.chars().count() <= 80,
                        "{line}"
                    );
                }
            }
        }
    }

    /// CyclePay's published terms: $1 to $200 an order.
    fn gateway(paused: bool) -> Gateway {
        Gateway::new(
            principal(GATEWAY),
            CardTerms {
                deliveries_paused: paused,
                min_order_usd_cents: 100,
                max_order_usd_cents: 20_000,
                fee_bps: 290,
                fee_fixed_usd_cents: 30,
                ledger_delivery_fee_cycles: Nat::from(200_000_000u64),
            },
        )
        .unwrap()
    }

    fn rate() -> Rate {
        Rate {
            xdr_usd_x10000: RATE,
            stale: false,
            age: None,
        }
    }

    /// What one card order does, at its edges: an order asked for 2% more
    /// than the hint's figure must still be one the gateway takes, so the
    /// most a hint asks of one order is what it buys, less that.
    #[test]
    fn one_card_order_with_room_for_the_rate() {
        let buys = ledger_cycles(20_000, RATE, &gateway(false).terms);
        let most = buys * 50 / 51;
        let order = |cycles| card_order(Ok((gateway(false), rate())), cycles);
        assert_eq!(order(2 * T), Cards::Cover);
        assert_eq!(order(most - 1_000_000), Cards::Cover);
        assert_eq!(order(most + 1_000_000), Cards::Short { most });
        // Within what one order buys, but not with the room.
        assert_eq!(order(buys - T), Cards::Short { most });
        assert_eq!(order(buys + T), Cards::Short { most });
        // A gateway taking no orders now is none to offer.
        assert_eq!(
            card_order(Ok((gateway(true), rate())), 2 * T),
            Cards::Unavailable
        );
    }

    /// A gateway that went unanswered may well answer next time; one that
    /// answered with no terms to sell on is none to offer.
    #[test]
    fn what_the_gateway_answered() {
        let gateway = principal(GATEWAY);
        let call = |error: CallError| {
            Box::new(TypedCallError::Call {
                source: Box::new(error),
            })
        };
        let unanswered = || {
            call(CallError::unanswered(
                gateway,
                CARD_TERMS_METHOD,
                std::io::Error::other("connection reset"),
            ))
        };
        let rejected = || {
            call(CallError::Rejected {
                canister: gateway,
                method: CARD_TERMS_METHOD.to_owned(),
                code: Some("IC0536".to_owned()),
                message: "no query method 'get_card_terms'".to_owned(),
            })
        };
        for (error, cards) in [
            (
                CyclesPurchaseError::ReadTerms {
                    gateway,
                    source: unanswered(),
                },
                Cards::Unknown,
            ),
            (
                CyclesPurchaseError::ReadRate {
                    gateway,
                    source: unanswered(),
                },
                Cards::Unknown,
            ),
            (
                CyclesPurchaseError::RateUnavailable { gateway },
                Cards::Unknown,
            ),
            (
                CyclesPurchaseError::ReadTerms {
                    gateway,
                    source: rejected(),
                },
                Cards::Unavailable,
            ),
            (
                CyclesPurchaseError::GatewayNotFound { gateway },
                Cards::Unavailable,
            ),
            (
                CyclesPurchaseError::InvalidTerms {
                    gateway,
                    why: "no order".to_owned(),
                },
                Cards::Unavailable,
            ),
        ] {
            assert_eq!(card_order(Err(error), 2 * T), cards);
        }
    }

    /// A gateway reached through calls that answer as `answer` says: with
    /// CyclePay's terms and rate, or never.
    struct FakeGateway {
        answer: bool,
    }

    #[async_trait]
    impl CanisterCalls for FakeGateway {
        fn caller(&self) -> Principal {
            principal(LATE)
        }

        async fn update(&self, _call: Call) -> Result<Vec<u8>, CallError> {
            unimplemented!()
        }

        async fn query(&self, call: Call) -> Result<Vec<u8>, CallError> {
            if !self.answer {
                pending::<()>().await;
            }
            Ok(if call.method == CARD_TERMS_METHOD {
                let terms = CardTerms {
                    deliveries_paused: false,
                    min_order_usd_cents: 100,
                    max_order_usd_cents: 20_000,
                    fee_bps: 290,
                    fee_fixed_usd_cents: 30,
                    ledger_delivery_fee_cycles: Nat::from(200_000_000u64),
                };
                Encode!(&terms).unwrap()
            } else {
                let rate = XdrRateStatus {
                    xdr_usd_x10000: RATE,
                    stale: false,
                    age_seconds: None,
                };
                Encode!(&rate).unwrap()
            })
        }

        async fn metadata_section(
            &self,
            _canister: Principal,
            _path: &str,
            _authority: Authority,
        ) -> Result<Option<Vec<u8>>, CallError> {
            unimplemented!()
        }

        async fn controllers(
            &self,
            _canister: Principal,
        ) -> Result<Option<Vec<Principal>>, CallError> {
            unimplemented!()
        }

        async fn module_hash(&self, _canister: Principal) -> Result<Option<Vec<u8>>, CallError> {
            unimplemented!()
        }

        async fn subnet_of(&self, _canister: Principal) -> Result<Principal, CallError> {
            unimplemented!()
        }

        async fn subnet_uses_engine_operator(&self, _subnet: Principal) -> Result<bool, CallError> {
            unimplemented!()
        }
    }

    /// The gateway's terms and rate are read and checked; a gateway that does
    /// not answer is waited on for no more than a few seconds.
    #[tokio::test(start_paused = true)]
    async fn the_gateway_is_asked_and_not_waited_on_long() {
        let gateway = principal(GATEWAY);
        let answering = FakeGateway { answer: true };
        assert_eq!(ask_gateway(&answering, gateway, 2 * T).await, Cards::Cover);
        assert!(matches!(
            ask_gateway(&answering, gateway, 500 * T).await,
            Cards::Short { .. }
        ));

        let silent = FakeGateway { answer: false };
        let start = tokio::time::Instant::now();
        assert_eq!(ask_gateway(&silent, gateway, 2 * T).await, Cards::Unknown);
        assert_eq!(start.elapsed(), CARD_TERMS_TIMEOUT);
        assert!(CARD_TERMS_TIMEOUT <= Duration::from_secs(5));
    }

    /// `command` split as the shells split it, for the quoting
    /// [`crate::commands::args::shell_word`] writes: runs in single or double
    /// quotes, with nothing escaped inside.
    fn shell_split(command: &str) -> Vec<String> {
        let (mut words, mut word, mut quote, mut any) = (Vec::new(), String::new(), None, false);
        for c in command.chars() {
            match (c, quote) {
                ('\'' | '"', None) => {
                    quote = Some(c);
                    any = true;
                }
                (c, Some(q)) if c == q => quote = None,
                (' ', None) => {
                    if any || !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                    any = false;
                }
                (c, _) => word.push(c),
            }
        }
        if any || !word.is_empty() {
            words.push(word);
        }
        words
    }

    /// The commands of a message, as printed on lines of their own.
    fn commands(said: &str) -> Vec<&str> {
        said.lines()
            .filter_map(|line| line.strip_prefix("    "))
            .collect()
    }

    /// Every command a hint prints parses as the CLI's own, asking for the
    /// cycles the hint names, as the same identity on the same network or
    /// environment the refused command ran with.
    #[test]
    fn the_suggested_commands_parse_with_the_same_selections() {
        let cases = [
            (named("alice"), NetworkSelection::Default, env("ic")),
            (
                named("my id"),
                NetworkSelection::Named("ic".into()),
                EnvironmentSelection::Default,
            ),
            (
                IdentitySelection::Default,
                NetworkSelection::Url("http://localhost:8000".parse().unwrap(), RootKeySpec::Fetch),
                EnvironmentSelection::Default,
            ),
            (
                named("o'neil"),
                NetworkSelection::Default,
                EnvironmentSelection::Default,
            ),
            (named("@team"), NetworkSelection::Default, env("ic")),
            (named("=eq"), NetworkSelection::Default, env("ic")),
            (named("-dash"), NetworkSelection::Default, env("-env")),
            (named("-my id"), NetworkSelection::Named("-net".into()), {
                EnvironmentSelection::Default
            }),
        ];
        for (identity, network, environment) in cases {
            let places = [
                Place::Mainnet(Cards::Cover),
                Place::Local {
                    caller: principal(LATE),
                },
                Place::Elsewhere,
            ];
            for place in places {
                let hint = Hint::new(
                    2 * T + FEE,
                    500 * B,
                    false,
                    place,
                    &identity,
                    &network,
                    &environment,
                )
                .unwrap();
                let said = hint.message("create 1 canister", DEPLOY, None);
                for command in commands(&said) {
                    let words = shell_split(command);
                    let cli =
                        Cli::try_parse_from(&words).unwrap_or_else(|e| panic!("{command}: {e}"));
                    // The transfer is the anonymous identity's, to the caller.
                    let (cycles, selections, from) = match cli.command {
                        Some(Command::Cycles(cycles::Command::Buy(args))) => (
                            args.cycles.map(|c| c.get()),
                            args.token_command_args.selections(),
                            identity.clone(),
                        ),
                        Some(Command::Cycles(cycles::Command::Mint(args))) => (
                            args.cycles.map(|c| c.get()),
                            args.token_command_args.selections(),
                            identity.clone(),
                        ),
                        Some(Command::Cycles(cycles::Command::Transfer(args))) => {
                            assert_eq!(args.receiver.owner, principal(LATE), "{command}");
                            assert_eq!(args.receiver.subaccount, None, "{command}");
                            (
                                Some(args.amount.get()),
                                args.token_command_args.selections(),
                                IdentitySelection::Anonymous,
                            )
                        }
                        _ => panic!("{command}: not icp cycles buy, mint or transfer"),
                    };
                    assert_eq!(cycles, Some(1_510_000_000_000), "{command}");
                    assert_eq!(selections.identity, from, "{command}");
                    assert_eq!(selections.network, network, "{command}");
                    assert_eq!(selections.environment, environment, "{command}");
                }
            }
        }
    }
}
