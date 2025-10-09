use icp::prelude::*;

use crate::common::TestContext;

pub mod cmc;
pub mod cycles_ledger;
pub mod icp_cli;
pub mod ledger;
pub mod registry;

pub fn icp(
    ctx: &TestContext,
    current_dir: impl Into<PathBuf>,
    environment: Option<String>,
) -> icp_cli::Client<'_> {
    icp_cli::Client::new(ctx, current_dir.into(), environment)
}

pub fn ledger(ctx: &TestContext) -> ledger::Client<'_> {
    ledger::Client::new(ctx)
}

pub fn cycles_ledger(ctx: &TestContext) -> cycles_ledger::Client<'_> {
    cycles_ledger::Client::new(ctx)
}

pub fn registry(ctx: &TestContext) -> registry::Client<'_> {
    registry::Client::new(ctx)
}

pub fn cmc(ctx: &TestContext) -> cmc::Client<'_> {
    cmc::Client::new(ctx)
}
