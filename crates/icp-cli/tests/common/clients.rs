use icp::prelude::*;

use crate::common::TestContext;

pub(crate) mod cmc;
pub(crate) mod cycles_ledger;
pub(crate) mod icp_cli;
pub(crate) mod ledger;
pub(crate) mod registry;

pub(crate) fn icp(
    ctx: &TestContext,
    current_dir: impl Into<PathBuf>,
    environment: Option<String>,
) -> icp_cli::Client<'_> {
    icp_cli::Client::new(ctx, current_dir.into(), environment)
}

pub(crate) fn ledger(ctx: &TestContext) -> ledger::Client<'_> {
    ledger::Client::new(ctx)
}

pub(crate) fn cycles_ledger(ctx: &TestContext) -> cycles_ledger::Client<'_> {
    cycles_ledger::Client::new(ctx)
}

pub(crate) fn registry(ctx: &TestContext) -> registry::Client<'_> {
    registry::Client::new(ctx)
}

pub(crate) fn cmc(ctx: &TestContext) -> cmc::Client<'_> {
    cmc::Client::new(ctx)
}
