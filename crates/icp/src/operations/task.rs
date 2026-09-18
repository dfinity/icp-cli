//! What a running operation is doing, and how it describes itself.
//!
//! Every kind of work the operations layer reports on is its own type
//! implementing [`Presentation`], so everything a frontend needs to know about
//! (say) a Candid check — its label while running, its success and failure
//! lines, the shape of its failure report — lives in one impl block instead of
//! being spread across one arm in each of half a dozen wording functions.
//!
//! This lives here rather than in a frontend because the operations construct
//! it and every frontend needs it: a terminal renderer, a `--json` stream, and
//! an in-browser caller would otherwise each re-invent the same descriptions.
//! What stays with the frontend is how these are drawn — colors, bar
//! templates, log decoration — and any wording naming a frontend's own
//! affordances (see [`Presentation::failure_is_bypassable`]).
//!
//! [`Task`] is the serializable envelope that travels on the event stream.
//! [`Task::presentation`] is the single dispatch point from envelope to
//! behavior, and it is exhaustive, so a new task cannot be added without
//! being wired up.

use candid::Principal;
use serde::Serialize;

/// The event types instantiated for this crate's task vocabulary.
/// Operations and frontends name these rather than repeating the
/// task parameter.
pub type Reporter = icp_events::Reporter<Task>;
pub type TaskReporter = icp_events::TaskReporter<Task>;
pub type Event = icp_events::Event<Task>;

/// The shape of live progress a task can report. How this is drawn — and
/// how the label is decorated — is the frontend's business.
pub enum Widget {
    /// No live progress of its own: an announcement that titles whatever
    /// nests beneath it. With nothing nested under it, it is just a notice.
    Heading { title: String },
    /// Work of unknown duration, labeled by the canister it acts on.
    Indeterminate { label: String },
    /// Quantifiable work: `total` bytes, labeled by what is being moved.
    Bytes { label: String, total: u64 },
}

/// Display data for a failed task, as carried by
/// [`icp_events::TaskOutcome::Failed`]. The typed error stays on the
/// operation's return path; this is only ever printed.
pub struct Failure {
    pub message: String,
    pub causes: Vec<String>,
}

/// How one kind of task presents itself. Defaults cover the common case — a
/// spinner-driven task that reports no steps — so most implementations only
/// state what makes them different.
pub trait Presentation {
    /// The canister this task operates on, when it is about one. Used to
    /// attribute captured output; a task that acts on no single canister —
    /// a phase heading — contributes no attribution.
    fn canister(&self) -> Option<&str>;

    /// The live widget this task drives.
    fn widget(&self) -> Widget {
        Widget::Indeterminate {
            label: self.canister().unwrap_or_default().to_owned(),
        }
    }

    /// Message shown while the task runs, before any step reports in.
    /// Step-reporting tasks return `None`: their step headers take over.
    fn running_message(&self) -> Option<&'static str> {
        None
    }

    /// Live header shown while a step runs. `label` may span multiple lines.
    ///
    /// Only tasks that actually report steps need to override this; the
    /// default passes the label through rather than inventing a header for a
    /// task that was never meant to have one.
    fn step_header(&self, _number: usize, _total: usize, label: &str) -> String {
        label.to_owned()
    }

    /// Label for the captured-output header, e.g. the "Build" in
    /// `[name] Build output:`. Only reachable for tasks that report steps.
    fn output_label(&self) -> &'static str {
        "Output"
    }

    /// Final widget message on success, or `None` for a widget with no
    /// message slot (a byte bar has none).
    fn success_message(&self) -> Option<String>;

    /// Final widget message on failure, or `None` as above.
    fn failure_message(&self, message: &str) -> Option<String>;

    /// First line of this task's deferred failure dump, or `None` for tasks
    /// whose failure travels on the command's returned error instead.
    fn failure_header(&self) -> Option<String> {
        None
    }

    /// The deferred failure dump for this task, ahead of any captured step
    /// output. `None` suppresses the dump entirely.
    fn failure_dump(&self, failure: &Failure) -> Option<Vec<String>> {
        let mut lines = vec![self.failure_header()?, format!("'{}'", failure.message)];
        lines.extend(
            failure
                .causes
                .iter()
                .map(|cause| format!("  caused by: {cause}")),
        );
        Some(lines)
    }

    /// Whether this failure is one the caller could have chosen to proceed
    /// through. Frontends use it to offer their own affordance for doing so
    /// — a CLI flag, a confirmation dialog — which is why the wording for
    /// that stays with the frontend rather than living here.
    fn failure_is_bypassable(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Phases
// ---------------------------------------------------------------------------

/// A step of a composite operation, titling the tasks nested under it.
///
/// A phase does no work of its own, so it has nothing to say on either
/// outcome: whichever child failed has already said what went wrong, and the
/// error itself is on the operation's return path.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "phase")]
pub struct PhaseTask {
    pub title: String,
}

impl Presentation for PhaseTask {
    /// A phase spans every canister the operation touches, so it attributes
    /// nothing to any one of them.
    fn canister(&self) -> Option<&str> {
        None
    }

    fn widget(&self) -> Widget {
        Widget::Heading {
            title: self.title.clone(),
        }
    }

    fn success_message(&self) -> Option<String> {
        None
    }

    fn failure_message(&self, _message: &str) -> Option<String> {
        None
    }
}

/// Announce something in passing: a phase with no work under it, finished as
/// soon as it is started.
pub fn notice(reporter: &Reporter, text: impl Into<String>) {
    reporter
        .task(Task::phase(text))
        .finish(icp_events::TaskOutcome::succeeded());
}

// ---------------------------------------------------------------------------
// Multi-step tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "build")]
pub struct BuildTask {
    pub canister: String,
}

impl Presentation for BuildTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn step_header(&self, number: usize, total: usize, label: &str) -> String {
        format!("Building: step {number} of {total} {label}")
    }

    fn output_label(&self) -> &'static str {
        "Build"
    }

    fn success_message(&self) -> Option<String> {
        Some("Built successfully".to_owned())
    }

    fn failure_message(&self, message: &str) -> Option<String> {
        Some(format!("Failed to build canister: {message}"))
    }

    fn failure_header(&self) -> Option<String> {
        Some(format!(
            "----- Failed to build canister '{}' -----",
            self.canister
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "sync")]
pub struct SyncTask {
    pub canister: String,
    pub canister_id: Principal,
}

impl Presentation for SyncTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn step_header(&self, number: usize, total: usize, label: &str) -> String {
        format!("\nSyncing: {label} {number} of {total}")
    }

    fn output_label(&self) -> &'static str {
        "Sync"
    }

    fn success_message(&self) -> Option<String> {
        Some(format!("Synced successfully: {}", self.canister_id))
    }

    fn failure_message(&self, message: &str) -> Option<String> {
        Some(format!("Failed to sync canister: {message}"))
    }

    fn failure_header(&self) -> Option<String> {
        Some(format!(
            "----- Failed to sync canister '{}': {} -----",
            self.canister, self.canister_id
        ))
    }
}

// ---------------------------------------------------------------------------
// Single-action tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "create")]
pub struct CreateTask {
    pub canister: String,
}

impl Presentation for CreateTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn running_message(&self) -> Option<&'static str> {
        Some("Creating...")
    }

    fn success_message(&self) -> Option<String> {
        Some("Created successfully".to_owned())
    }

    /// Create failures surface through the command's returned error, so the
    /// bar shows the bare message and there is no deferred dump.
    fn failure_message(&self, message: &str) -> Option<String> {
        Some(message.to_owned())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "install")]
pub struct InstallTask {
    pub canister: String,
    pub canister_id: Principal,
}

impl Presentation for InstallTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn running_message(&self) -> Option<&'static str> {
        Some("Installing...")
    }

    fn success_message(&self) -> Option<String> {
        Some("Installed successfully".to_owned())
    }

    fn failure_message(&self, message: &str) -> Option<String> {
        Some(format!("Failed to install canister: {message}"))
    }

    fn failure_header(&self) -> Option<String> {
        Some(format!(
            "----- Failed to install canister '{}': {} -----",
            self.canister, self.canister_id
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "update_settings")]
pub struct UpdateSettingsTask {
    pub canister: String,
    pub canister_id: Principal,
}

impl Presentation for UpdateSettingsTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn running_message(&self) -> Option<&'static str> {
        Some("Updating canister settings...")
    }

    fn success_message(&self) -> Option<String> {
        Some("Canister settings updated successfully".to_owned())
    }

    fn failure_message(&self, message: &str) -> Option<String> {
        Some(format!("Failed to update canister settings: {message}"))
    }

    fn failure_header(&self) -> Option<String> {
        Some(format!(
            "----- Failed to update settings for canister '{}': {} -----",
            self.canister, self.canister_id
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "update_environment_variables")]
pub struct UpdateEnvironmentVariablesTask {
    pub canister: String,
    pub canister_id: Principal,
}

impl Presentation for UpdateEnvironmentVariablesTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn running_message(&self) -> Option<&'static str> {
        Some("Updating environment variables...")
    }

    fn success_message(&self) -> Option<String> {
        Some("Environment variables updated successfully".to_owned())
    }

    fn failure_message(&self, message: &str) -> Option<String> {
        Some(format!("Failed to update environment variables: {message}"))
    }

    fn failure_header(&self) -> Option<String> {
        Some(format!(
            "----- Failed to update environment variables for canister '{}': {} -----",
            self.canister, self.canister_id
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "candid_check")]
pub struct CandidCheckTask {
    pub canister: String,
    pub canister_id: Principal,
}

impl Presentation for CandidCheckTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn running_message(&self) -> Option<&'static str> {
        Some("Checking compatibility...")
    }

    fn success_message(&self) -> Option<String> {
        Some("Compatible".to_owned())
    }

    /// The incompatibility details ride on the failure message but are far
    /// too long for a progress bar; the dump below prints them instead.
    fn failure_message(&self, _message: &str) -> Option<String> {
        Some("Incompatible".to_owned())
    }

    fn failure_header(&self) -> Option<String> {
        Some(format!(
            " ----- Candid interface compatibility check failed: '{}' ({}) -----",
            self.canister, self.canister_id
        ))
    }

    /// A breaking change gets its own wording, and no cause chain: the
    /// message is the rendered incompatibility report.
    fn failure_dump(&self, failure: &Failure) -> Option<Vec<String>> {
        Some(vec![
            self.failure_header()?,
            format!(
                "You are making a BREAKING change. Other canisters or frontend clients \
                 relying on your canister may stop working.\n\n{}",
                failure.message,
            ),
        ])
    }

    fn failure_is_bypassable(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Byte transfers
// ---------------------------------------------------------------------------

/// Which way a snapshot blob is moving relative to the local machine.
/// Carried for the benefit of the event stream; the terminal renderers label
/// transfers by blob alone.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Upload,
    Download,
}

/// The snapshot blob being transferred.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferBlob {
    WasmModule,
    WasmMemory,
    StableMemory,
}

impl TransferBlob {
    fn label(self) -> &'static str {
        match self {
            TransferBlob::WasmModule => "WASM module",
            TransferBlob::WasmMemory => "WASM memory",
            TransferBlob::StableMemory => "Stable memory",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename = "snapshot_transfer")]
pub struct SnapshotTransferTask {
    pub canister: String,
    pub direction: TransferDirection,
    pub blob: TransferBlob,
    pub total_bytes: u64,
}

impl Presentation for SnapshotTransferTask {
    fn canister(&self) -> Option<&str> {
        Some(&self.canister)
    }

    fn widget(&self) -> Widget {
        Widget::Bytes {
            label: self.blob.label().to_owned(),
            total: self.total_bytes,
        }
    }

    /// A byte bar's template has no message slot, so there is nothing to say
    /// on either outcome — it simply freezes at its final position. Transfer
    /// failures surface through the command's returned error.
    fn success_message(&self) -> Option<String> {
        None
    }

    fn failure_message(&self, _message: &str) -> Option<String> {
        None
    }
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// One unit of work the CLI reports on. This is the payload carried by
/// `icp_events::EventKind::TaskStarted`.
///
/// `untagged` defers to each task's own internally-tagged representation, so
/// a build serializes as `{"kind":"build","canister":"..."}`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Task {
    Phase(PhaseTask),
    Build(BuildTask),
    Sync(SyncTask),
    Create(CreateTask),
    Install(InstallTask),
    UpdateSettings(UpdateSettingsTask),
    UpdateEnvironmentVariables(UpdateEnvironmentVariablesTask),
    CandidCheck(CandidCheckTask),
    SnapshotTransfer(SnapshotTransferTask),
}

impl Task {
    /// The one dispatch point from the serializable envelope to the
    /// per-task presentation rules.
    pub fn presentation(&self) -> &dyn Presentation {
        match self {
            Task::Phase(task) => task,
            Task::Build(task) => task,
            Task::Sync(task) => task,
            Task::Create(task) => task,
            Task::Install(task) => task,
            Task::UpdateSettings(task) => task,
            Task::UpdateEnvironmentVariables(task) => task,
            Task::CandidCheck(task) => task,
            Task::SnapshotTransfer(task) => task,
        }
    }

    pub fn phase(title: impl Into<String>) -> Self {
        Task::Phase(PhaseTask {
            title: title.into(),
        })
    }

    pub fn build(canister: impl Into<String>) -> Self {
        Task::Build(BuildTask {
            canister: canister.into(),
        })
    }

    pub fn sync(canister: impl Into<String>, canister_id: Principal) -> Self {
        Task::Sync(SyncTask {
            canister: canister.into(),
            canister_id,
        })
    }

    pub fn create(canister: impl Into<String>) -> Self {
        Task::Create(CreateTask {
            canister: canister.into(),
        })
    }

    pub fn install(canister: impl Into<String>, canister_id: Principal) -> Self {
        Task::Install(InstallTask {
            canister: canister.into(),
            canister_id,
        })
    }

    pub fn update_settings(canister: impl Into<String>, canister_id: Principal) -> Self {
        Task::UpdateSettings(UpdateSettingsTask {
            canister: canister.into(),
            canister_id,
        })
    }

    pub fn update_environment_variables(
        canister: impl Into<String>,
        canister_id: Principal,
    ) -> Self {
        Task::UpdateEnvironmentVariables(UpdateEnvironmentVariablesTask {
            canister: canister.into(),
            canister_id,
        })
    }

    pub fn candid_check(canister: impl Into<String>, canister_id: Principal) -> Self {
        Task::CandidCheck(CandidCheckTask {
            canister: canister.into(),
            canister_id,
        })
    }

    pub fn snapshot_transfer(
        canister: impl Into<String>,
        direction: TransferDirection,
        blob: TransferBlob,
        total_bytes: u64,
    ) -> Self {
        Task::SnapshotTransfer(SnapshotTransferTask {
            canister: canister.into(),
            direction,
            blob,
            total_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid() -> Principal {
        Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap()
    }

    /// The task payload is the one part of the event stream whose shape this
    /// crate owns, and the `--json` renderer planned in #493 will publish it.
    /// Each variant serializes as its own internally-tagged object, so the
    /// envelope adds no nesting of its own.
    #[test]
    fn task_wire_format_is_stable() {
        let json = |task: Task| serde_json::to_value(task).expect("task should serialize");

        assert_eq!(
            json(Task::phase("Building canisters:")),
            serde_json::json!({ "kind": "phase", "title": "Building canisters:" })
        );
        assert_eq!(
            json(Task::build("frontend")),
            serde_json::json!({ "kind": "build", "canister": "frontend" })
        );
        assert_eq!(
            json(Task::sync("frontend", cid())),
            serde_json::json!({
                "kind": "sync", "canister": "frontend",
                "canister_id": "rrkah-fqaaa-aaaaa-aaaaq-cai",
            })
        );
        assert_eq!(
            json(Task::create("frontend")),
            serde_json::json!({ "kind": "create", "canister": "frontend" })
        );
        assert_eq!(
            json(Task::install("frontend", cid())),
            serde_json::json!({
                "kind": "install", "canister": "frontend",
                "canister_id": "rrkah-fqaaa-aaaaa-aaaaq-cai",
            })
        );
        assert_eq!(
            json(Task::update_settings("frontend", cid())),
            serde_json::json!({
                "kind": "update_settings", "canister": "frontend",
                "canister_id": "rrkah-fqaaa-aaaaa-aaaaq-cai",
            })
        );
        assert_eq!(
            json(Task::update_environment_variables("frontend", cid())),
            serde_json::json!({
                "kind": "update_environment_variables", "canister": "frontend",
                "canister_id": "rrkah-fqaaa-aaaaa-aaaaq-cai",
            })
        );
        assert_eq!(
            json(Task::candid_check("frontend", cid())),
            serde_json::json!({
                "kind": "candid_check", "canister": "frontend",
                "canister_id": "rrkah-fqaaa-aaaaa-aaaaq-cai",
            })
        );
        assert_eq!(
            json(Task::snapshot_transfer(
                "frontend",
                TransferDirection::Download,
                TransferBlob::StableMemory,
                4096,
            )),
            serde_json::json!({
                "kind": "snapshot_transfer", "canister": "frontend",
                "direction": "download", "blob": "stable_memory", "total_bytes": 4096,
            })
        );
    }

    /// Every task must be reachable through the envelope's dispatch, and must
    /// agree with it on which canister it is about.
    #[test]
    fn every_task_dispatches_to_its_own_presentation() {
        let tasks = [
            Task::build("c"),
            Task::sync("c", cid()),
            Task::create("c"),
            Task::install("c", cid()),
            Task::update_settings("c", cid()),
            Task::update_environment_variables("c", cid()),
            Task::candid_check("c", cid()),
            Task::snapshot_transfer("c", TransferDirection::Upload, TransferBlob::WasmModule, 1),
        ];

        for task in &tasks {
            assert_eq!(task.presentation().canister(), Some("c"));
        }

        // A phase spans all of them, so it names none.
        assert_eq!(
            Task::phase("Building canisters:").presentation().canister(),
            None
        );
    }

    /// A phase is a heading and nothing else: no live state, no closing line
    /// on either outcome, and no deferred dump — whichever child failed has
    /// already reported, and the error is on the return path.
    #[test]
    fn a_phase_is_a_heading_with_nothing_to_say() {
        let phase = Task::phase("Installing canisters:");
        let presentation = phase.presentation();

        match presentation.widget() {
            Widget::Heading { title } => assert_eq!(title, "Installing canisters:"),
            _ => panic!("a phase must render as a heading"),
        }
        assert!(presentation.success_message().is_none());
        assert!(presentation.failure_message("boom").is_none());
        assert!(
            presentation
                .failure_dump(&Failure {
                    message: "boom".to_owned(),
                    causes: vec!["inner".to_owned()],
                })
                .is_none()
        );
    }

    /// Only the two multi-step kinds render step headers and captured-output
    /// labels; the rest never report steps, so they keep the neutral default
    /// rather than borrowing the build wording.
    #[test]
    fn step_wording_belongs_to_the_multi_step_kinds() {
        let build = Task::build("c");
        assert_eq!(
            build.presentation().step_header(1, 3, "script"),
            "Building: step 1 of 3 script"
        );
        assert_eq!(build.presentation().output_label(), "Build");

        let sync = Task::sync("c", cid());
        assert_eq!(
            sync.presentation().step_header(2, 4, "asset"),
            "\nSyncing: asset 2 of 4"
        );
        assert_eq!(sync.presentation().output_label(), "Sync");

        let install = Task::install("c", cid());
        assert_eq!(install.presentation().step_header(1, 1, "unused"), "unused");
        assert_eq!(install.presentation().output_label(), "Output");
    }

    /// A byte bar has no message slot, which is how the renderer knows to
    /// leave the bar alone on completion instead of stamping a tick on it.
    #[test]
    fn byte_transfers_have_no_completion_message() {
        let transfer =
            Task::snapshot_transfer("c", TransferDirection::Upload, TransferBlob::WasmMemory, 8);
        assert!(transfer.presentation().success_message().is_none());
        assert!(transfer.presentation().failure_message("boom").is_none());
        assert!(transfer.presentation().failure_header().is_none());

        match transfer.presentation().widget() {
            Widget::Bytes { label, total } => {
                assert_eq!(label, "WASM memory");
                assert_eq!(total, 8);
            }
            _ => panic!("a transfer must use a byte bar"),
        }
    }

    /// Create and transfer failures travel on the command's returned error,
    /// so they must not also produce a deferred dump.
    #[test]
    fn kinds_without_a_header_produce_no_dump() {
        let failure = Failure {
            message: "boom".to_owned(),
            causes: vec!["inner".to_owned()],
        };

        for task in [
            Task::create("c"),
            Task::snapshot_transfer("c", TransferDirection::Upload, TransferBlob::WasmModule, 1),
        ] {
            assert!(task.presentation().failure_dump(&failure).is_none());
        }
    }

    #[test]
    fn failure_dump_carries_the_message_then_its_cause_chain() {
        let failure = Failure {
            message: "outer".to_owned(),
            causes: vec!["middle".to_owned(), "inner".to_owned()],
        };

        assert_eq!(
            Task::sync("c", cid())
                .presentation()
                .failure_dump(&failure)
                .expect("sync failures dump"),
            vec![
                "----- Failed to sync canister 'c': rrkah-fqaaa-aaaaa-aaaaq-cai -----".to_owned(),
                "'outer'".to_owned(),
                "  caused by: middle".to_owned(),
                "  caused by: inner".to_owned(),
            ]
        );
    }

    /// A Candid failure replaces the generic dump with the breaking-change
    /// warning, and is the only kind the caller may choose to proceed past.
    #[test]
    fn candid_failures_get_their_own_wording_and_are_bypassable() {
        let failure = Failure {
            message: "method foo removed".to_owned(),
            causes: vec!["ignored".to_owned()],
        };
        let task = Task::candid_check("c", cid());

        let dump = task
            .presentation()
            .failure_dump(&failure)
            .expect("candid failures dump");
        assert_eq!(dump.len(), 2, "header plus the breaking-change warning");
        assert!(dump[0].contains("Candid interface compatibility check failed: 'c'"));
        assert!(dump[1].starts_with("You are making a BREAKING change."));
        assert!(dump[1].ends_with("method foo removed"));
        // The cause chain is deliberately dropped: the message is the report.
        assert!(!dump[1].contains("ignored"));

        assert!(task.presentation().failure_is_bypassable());
        assert!(
            !Task::build("c").presentation().failure_is_bypassable(),
            "a build failure is not something the caller can proceed past"
        );
    }
}
