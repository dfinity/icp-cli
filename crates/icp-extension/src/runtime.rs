// Host-side Component Model runtime for extensions.
use std::sync::Arc;

use candid::Principal;
use icp_project::calls::{Call, CanisterCalls};
use icp_project::error;
use semver::{Version, VersionReq};
use snafu::prelude::*;
use url::Url;
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store};

use crate::{AllowedCanister, Info, Matched, RunContext};

// Maximum wasm call-stack depth (in bytes).
const MAX_WASM_STACK: usize = 512 * 1024;

mod v1 {
    wasmtime::component::bindgen!({
        world: "extension",
        path: "extension.wit",
    });
}

use v1::icp::extension::types as wit;

/// How an extension talks to the person at the terminal. Implemented by the
/// CLI, which owns all presentation.
pub trait Terminal: Send + Sync {
    /// Ask a yes/no question; return `default` when nobody is there to ask.
    fn confirm(&self, prompt: &str, default: bool) -> bool;
    /// Show the user a web page to visit: always an absolute http(s) URL.
    fn open_url(&self, url: &Url);
}

/// A terminal that answers nothing: what `describe` runs against, since an
/// extension describing itself has nobody to talk to.
struct Silent;

impl Terminal for Silent {
    fn confirm(&self, _prompt: &str, default: bool) -> bool {
        default
    }
    fn open_url(&self, _url: &Url) {}
}

struct HostState {
    /// How calls are made; `None` when the command asked for no network.
    calls: Option<Arc<dyn CanisterCalls>>,
    /// Whether the network is mainnet, where calls are held to `allowed`.
    mainnet: bool,
    allowed: Vec<AllowedCanister>,
    terminal: Arc<dyn Terminal>,
    wasi_ctx: wasmtime_wasi::WasiCtx,
    wasi_table: wasmtime_wasi::ResourceTable,
}

impl wasmtime_wasi::WasiView for HostState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.wasi_table,
        }
    }
}

impl HostState {
    /// Which canister a request may reach: any on a network that is not
    /// mainnet, and on mainnet only those the extension declared. Never the
    /// management canister: a call to it acts on the canister its argument
    /// names, which neither the allowlist nor the call's routing can see.
    fn permitted(&self, canister: &str) -> Result<Principal, String> {
        let principal = Principal::from_text(canister)
            .map_err(|e| format!("'{canister}' is not a valid principal: {e}"))?;
        if principal == Principal::management_canister() {
            return Err("extensions may not call the management canister".into());
        }
        if self.mainnet && !self.allowed.iter().any(|a| a.id == principal) {
            return Err(format!(
                "the extension may not call {principal} on mainnet: it is not among the \
                 canisters approved at install"
            ));
        }
        Ok(principal)
    }
}

impl wit::Host for HostState {}

impl v1::ExtensionImports for HostState {
    fn canister_call(&mut self, req: wit::CanisterCallRequest) -> Result<Vec<u8>, String> {
        let Some(calls) = self.calls.clone() else {
            return Err(
                "this command has no network to call: it does not ask for `network`".into(),
            );
        };
        let canister = self.permitted(&req.canister)?;
        let call = Call::new(canister, req.method, req.arg).direct();
        // We are inside `tokio::task::block_in_place` (see `run`), so
        // blocking the thread here is safe.
        tokio::runtime::Handle::current().block_on(async move {
            match req.call_type {
                wit::CallType::Update => calls.update(call).await,
                wit::CallType::Query => calls.query(call).await,
            }
            .map_err(|err| error::flatten(&err))
        })
    }

    fn confirm(&mut self, prompt: String, default: bool) -> bool {
        self.terminal.confirm(&prompt, default)
    }

    fn open_url(&mut self, url: String) -> Result<(), String> {
        let url = web_page(&url)?;
        self.terminal.open_url(&url);
        Ok(())
    }
}

/// `url`, if it is an absolute http(s) URL: what `open-url` may show. The
/// opener behind it would as readily launch a local file or a custom scheme's
/// handler, which is the user's machine rather than a page.
fn web_page(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|e| format!("'{url}' is not a URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" if parsed.has_host() => Ok(parsed),
        _ => Err(format!(
            "'{url}' is not an http(s) URL; only web pages can be opened"
        )),
    }
}

#[derive(Debug, Snafu)]
pub enum DescribeError {
    #[snafu(transparent)]
    Load { source: LoadError },

    #[snafu(display("failed to call describe() on the extension"))]
    CallDescribe { source: wasmtime::Error },

    #[snafu(display("the extension declares '{id}' as a canister, which is not a principal"))]
    CanisterId {
        id: String,
        source: candid::types::principal::PrincipalError,
    },
}

#[derive(Debug, Snafu)]
pub enum RunError {
    #[snafu(transparent)]
    Load { source: LoadError },

    #[snafu(display("failed to call run() on the extension"))]
    CallRun { source: wasmtime::Error },

    #[snafu(display("{message}"))]
    ExtensionFailed { message: String },
}

#[derive(Debug, Snafu)]
pub enum LoadError {
    #[snafu(display("failed to create the wasmtime engine"))]
    CreateEngine { source: wasmtime::Error },

    #[snafu(display("failed to load the extension's wasm component"))]
    LoadComponent { source: wasmtime::Error },

    #[snafu(display(
        "the wasm component does not implement a supported extension interface ({detail}). \
         Supported: icp:extension@0.1."
    ))]
    UnsupportedInterface { detail: String },

    #[snafu(display("failed to instantiate the extension"))]
    Instantiate { source: wasmtime::Error },
}

/// The interface package an extension imports, whose version picks the ABI.
const TYPES_INTERFACE_PREFIX: &str = "icp:extension/types@";

/// Load and instantiate an extension component from its bytes.
fn instantiate(
    wasm: &[u8],
    state: HostState,
) -> Result<(Store<HostState>, v1::Extension), LoadError> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.max_wasm_stack(MAX_WASM_STACK);
    // Linear memory is bounded by the wasm32 address space. There is no
    // compute limit: an extension is interactive, may wait minutes on
    // something it is polling, and ends with Ctrl-C like any command.
    let engine = Engine::new(&config).context(CreateEngineSnafu)?;
    let component = Component::new(&engine, wasm).context(LoadComponentSnafu)?;

    // Read the interface version off the component's own imports, as the
    // sync-plugin runtime does, so that a future @0.2 can be served beside
    // @0.1.
    let raw = component
        .component_type()
        .imports(&engine)
        .find_map(|(name, _)| name.strip_prefix(TYPES_INTERFACE_PREFIX).map(str::to_owned))
        .context(UnsupportedInterfaceSnafu {
            detail: format!("no {TYPES_INTERFACE_PREFIX}<version> import found"),
        })?;
    let version = Version::parse(&raw).map_err(|e| {
        UnsupportedInterfaceSnafu {
            detail: format!("interface version '{raw}' is not valid semver: {e}"),
        }
        .build()
    })?;
    ensure!(
        VersionReq::parse("^0.1")
            .expect("valid req")
            .matches(&version),
        UnsupportedInterfaceSnafu {
            detail: format!("unsupported interface version {version}"),
        }
    );

    let mut linker: Linker<HostState> = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).context(InstantiateSnafu)?;
    v1::Extension::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s).context(InstantiateSnafu)?;
    let mut store = Store::new(&engine, state);
    let extension =
        v1::Extension::instantiate(&mut store, &component, &linker).context(InstantiateSnafu)?;
    Ok((store, extension))
}

/// The sandbox: stdout and stderr reach the terminal, and the clocks and
/// randomness WASI provides by default are there. No preopened directory, no
/// environment variable, no argument, no stdin, and no socket address is
/// allowed.
fn wasi(inherit_output: bool) -> wasmtime_wasi::WasiCtx {
    let mut builder = wasmtime_wasi::WasiCtxBuilder::new();
    if inherit_output {
        builder.inherit_stdout().inherit_stderr();
    }
    builder.build()
}

/// Ask an extension what it is. Every import refuses or answers nothing, and
/// what it prints is discarded.
pub fn describe(wasm: &[u8]) -> Result<Info, DescribeError> {
    let state = HostState {
        calls: None,
        mainnet: true,
        allowed: vec![],
        terminal: Arc::new(Silent),
        wasi_ctx: wasi(false),
        wasi_table: wasmtime_wasi::ResourceTable::new(),
    };
    let (mut store, extension) = instantiate(wasm, state)?;
    let info = extension
        .call_describe(&mut store)
        .context(CallDescribeSnafu)?;
    let canisters = info
        .canisters
        .into_iter()
        .map(|c| {
            Ok(AllowedCanister {
                id: Principal::from_text(&c.id).context(CanisterIdSnafu { id: &c.id })?,
                label: c.label,
            })
        })
        .collect::<Result<_, DescribeError>>()?;
    Ok(Info {
        name: info.name,
        version: info.version,
        summary: info.summary,
        commands: info.commands.into_iter().map(command_from_wit).collect(),
        canisters,
    })
}

/// Run one command of an extension.
///
/// `calls` is how canister calls are made, as the resolved identity on the
/// resolved network; `None` when the command asked for no network. On
/// mainnet (`ctx.mainnet`) calls are held to `allowed`.
pub async fn run(
    wasm: Vec<u8>,
    allowed: Vec<AllowedCanister>,
    cmd: Matched,
    ctx: RunContext,
    calls: Option<Arc<dyn CanisterCalls>>,
    terminal: Arc<dyn Terminal>,
) -> Result<(), RunError> {
    // The runtime is synchronous — a wasm import cannot suspend — and it
    // blocks on the calls the extension makes, so it needs a thread it is
    // allowed to occupy.
    tokio::task::block_in_place(move || {
        let state = HostState {
            calls,
            mainnet: ctx.mainnet,
            allowed,
            terminal,
            wasi_ctx: wasi(true),
            wasi_table: wasmtime_wasi::ResourceTable::new(),
        };
        let (mut store, extension) = instantiate(&wasm, state)?;
        let matched = wit::Matched {
            path: cmd.path,
            values: cmd
                .values
                .into_iter()
                .map(|(name, values)| wit::ArgValue { name, values })
                .collect(),
        };
        let context = wit::Context {
            identity_principal: ctx.identity_principal.map(|p| p.to_text()),
            network: ctx.network,
            mainnet: ctx.mainnet,
            environment: ctx.environment,
            canister_ids: ctx
                .canister_ids
                .into_iter()
                .map(|(name, id)| wit::CanisterIdEntry {
                    name,
                    id: id.to_text(),
                })
                .collect(),
            interactive: ctx.interactive,
        };
        extension
            .call_run(&mut store, &matched, &context)
            .context(CallRunSnafu)?
            .map_err(|message| RunError::ExtensionFailed { message })
    })
}

fn command_from_wit(c: wit::Command) -> crate::Command {
    crate::Command {
        path: c.path,
        about: c.about,
        args: c
            .args
            .into_iter()
            .map(|a| crate::Arg {
                name: a.name,
                long: a.long,
                short: a.short,
                kind: match a.kind {
                    wit::ArgKind::Flag => crate::ArgKind::Flag,
                    wit::ArgKind::Value => crate::ArgKind::Value,
                    wit::ArgKind::Values => crate::ArgKind::Values,
                },
                required: a.required,
                default_value: a.default_value,
                possible_values: a.possible_values,
                value_name: a.value_name,
                help: a.help,
            })
            .collect(),
        standard: c
            .standard
            .into_iter()
            .map(|s| match s {
                wit::StandardOpt::Identity => crate::StandardOpt::Identity,
                wit::StandardOpt::Network => crate::StandardOpt::Network,
            })
            .collect(),
        default_network: c.default_network,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use icp_project::calls::{Authority, CallError};

    use super::*;

    fn fixture() -> Option<Vec<u8>> {
        let Some(path) = option_env!("HELLO_EXTENSION_WASM") else {
            eprintln!("skipping: wasm32-wasip2 not installed, so the fixture was not built");
            return None;
        };
        Some(std::fs::read(path).unwrap())
    }

    #[derive(Default)]
    struct Recorder {
        answer: bool,
        opened: Mutex<Vec<String>>,
    }

    impl Terminal for Recorder {
        fn confirm(&self, _prompt: &str, _default: bool) -> bool {
            self.answer
        }
        fn open_url(&self, url: &Url) {
            self.opened.lock().unwrap().push(url.to_string());
        }
    }

    /// Answers every query with a fixed reply, and remembers whom it called.
    #[derive(Default)]
    struct Echo {
        called: Mutex<Vec<Principal>>,
    }

    #[async_trait]
    impl CanisterCalls for Echo {
        fn caller(&self) -> Principal {
            Principal::anonymous()
        }
        async fn update(&self, call: Call) -> Result<Vec<u8>, CallError> {
            self.query(call).await
        }
        async fn query(&self, call: Call) -> Result<Vec<u8>, CallError> {
            self.called.lock().unwrap().push(call.canister);
            Ok(b"DIDL\x00\x00".to_vec())
        }
        async fn metadata_section(
            &self,
            _: Principal,
            _: &str,
            _: Authority,
        ) -> Result<Option<Vec<u8>>, CallError> {
            unimplemented!()
        }
        async fn controllers(&self, _: Principal) -> Result<Option<Vec<Principal>>, CallError> {
            unimplemented!()
        }
        async fn module_hash(&self, _: Principal) -> Result<Option<Vec<u8>>, CallError> {
            unimplemented!()
        }
        async fn subnet_of(&self, _: Principal) -> Result<Principal, CallError> {
            unimplemented!()
        }
        async fn subnet_uses_engine_operator(&self, _: Principal) -> Result<bool, CallError> {
            unimplemented!()
        }
    }

    const LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
    const OTHER: &str = "lqy7q-dh777-77777-aaaaq-cai";

    fn matched(path: &[&str], values: &[(&str, &[&str])]) -> Matched {
        Matched {
            path: path.iter().map(|s| s.to_string()).collect(),
            values: values
                .iter()
                .map(|(n, v)| (n.to_string(), v.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    fn call(canister: &str) -> Matched {
        matched(&["call"], &[("canister", &[canister]), ("method", &["m"])])
    }

    #[test]
    fn describe_reads_what_the_extension_declares() {
        let Some(wasm) = fixture() else { return };
        let info = describe(&wasm).unwrap();
        assert_eq!(info.name, "hello");
        assert_eq!(info.version, "0.1.0");
        let paths: Vec<_> = info.commands.iter().map(|c| c.path.join(" ")).collect();
        assert_eq!(paths, ["greet", "whoami", "call", "ask", "fail"]);
        assert_eq!(info.canisters[0].id.to_text(), LEDGER);
        crate::validate(&info).unwrap();
    }

    #[test]
    fn a_component_that_is_not_an_extension_is_refused() {
        let error = describe(b"not wasm").unwrap_err();
        assert!(matches!(
            error,
            DescribeError::Load {
                source: LoadError::LoadComponent { .. }
            }
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_extension_error_is_reported_as_its_message() {
        let Some(wasm) = fixture() else { return };
        let error = run(
            wasm,
            vec![],
            matched(&["fail"], &[]),
            RunContext::default(),
            None,
            Arc::new(Silent),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "deliberate failure");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn confirm_and_open_url_reach_the_terminal() {
        let Some(wasm) = fixture() else { return };
        let terminal = Arc::new(Recorder {
            answer: true,
            ..Default::default()
        });
        run(
            wasm,
            vec![],
            matched(&["ask"], &[]),
            RunContext::default(),
            None,
            terminal.clone(),
        )
        .await
        .unwrap();
        assert_eq!(*terminal.opened.lock().unwrap(), ["https://example.com/"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_a_web_page_is_opened() {
        let Some(wasm) = fixture() else { return };
        let terminal = Arc::new(Recorder {
            answer: true,
            ..Default::default()
        });
        for url in [
            "file:///etc/passwd",
            "/etc/passwd",
            "vscode://x",
            "https://",
        ] {
            let error = run(
                wasm.clone(),
                vec![],
                matched(&["ask"], &[("url", &[url])]),
                RunContext::default(),
                None,
                terminal.clone(),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("URL"), "{url}: {error}");
        }
        assert!(terminal.opened.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_management_canister_is_never_called() {
        let Some(wasm) = fixture() else { return };
        let echo = Arc::new(Echo::default());
        let error = run(
            wasm,
            vec![],
            call("aaaaa-aa"),
            RunContext::default(),
            Some(echo.clone()),
            Arc::new(Silent),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("management canister"), "{error}");
        assert!(echo.called.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_command_without_a_network_cannot_call() {
        let Some(wasm) = fixture() else { return };
        let error = run(
            wasm,
            vec![],
            call(LEDGER),
            RunContext::default(),
            None,
            Arc::new(Silent),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("no network"), "{error}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn on_mainnet_only_approved_canisters_are_called() {
        let Some(wasm) = fixture() else { return };
        let allowed = describe(&wasm).unwrap().canisters;
        let ctx = RunContext {
            mainnet: true,
            ..Default::default()
        };
        let echo = Arc::new(Echo::default());

        run(
            wasm.clone(),
            allowed.clone(),
            call(LEDGER),
            ctx.clone(),
            Some(echo.clone()),
            Arc::new(Silent),
        )
        .await
        .unwrap();
        let error = run(
            wasm,
            allowed,
            call(OTHER),
            ctx,
            Some(echo.clone()),
            Arc::new(Silent),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("approved at install"), "{error}");
        assert_eq!(
            *echo.called.lock().unwrap(),
            [Principal::from_text(LEDGER).unwrap()]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn elsewhere_any_canister_is_called() {
        let Some(wasm) = fixture() else { return };
        let echo = Arc::new(Echo::default());
        run(
            wasm,
            vec![],
            call(OTHER),
            RunContext::default(),
            Some(echo.clone()),
            Arc::new(Silent),
        )
        .await
        .unwrap();
        assert_eq!(
            *echo.called.lock().unwrap(),
            [Principal::from_text(OTHER).unwrap()]
        );
    }
}
