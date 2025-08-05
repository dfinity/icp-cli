use mockall::automock;
use snafu::Snafu;

use crate::{BuildSteps, Recipe, SyncSteps};

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(display("failed to resolve recipe into build/sync steps"))]
    Resolve,
}

/// A recipe resolver takes a recipe that is specified in a canister manifest
/// and resolves it into a set of build/sync steps
#[automock]
pub trait Resolve: Sync + Send {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError>;
}

pub struct Resolver;

impl Resolve for Resolver {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        // Build
        let build = BuildSteps { steps: vec![] };

        // Sync
        let sync = SyncSteps { steps: vec![] };

        Ok((build, sync))
    }
}
