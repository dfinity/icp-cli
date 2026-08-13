//! The CLI's execution context.
//!
//! Wraps the library [`icp::context::Context`] — which is a bag of ports for
//! building and deploying — with the frontend-only state the library has no
//! business knowing about, such as presentation flags. Derefs to the library
//! context, so every library port (`dirs`, `ids`, `project`, `network`, …) is
//! reached straight through it.

use std::{ops::Deref, time::Duration};

use icp::{context::ContextInitError, identity::PasswordFunc, prelude::*};

/// Execution context for a single CLI invocation.
#[derive(Clone)]
pub struct Context {
    /// The library context.
    inner: icp::context::Context,

    /// Whether debug output is enabled (`--debug`). Presentation only: it
    /// selects the tracing layer and hides progress bars.
    pub debug: bool,
}

impl Deref for Context {
    type Target = icp::context::Context;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Builds the context for this CLI invocation.
pub fn initialize(
    project_root_override: Option<PathBuf>,
    debug: bool,
    password_func: PasswordFunc,
    pem_session_duration: Option<Duration>,
) -> Result<Context, ContextInitError> {
    let inner =
        icp::context::initialize(project_root_override, password_func, pem_session_duration)?;

    Ok(Context { inner, debug })
}
