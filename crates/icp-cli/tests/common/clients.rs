use cycles_ledger::CyclesLedgerPocketIcClient;
use icp::prelude::*;
pub use icp_cli::IcpCliClient;
use icp_ledger::IcpLedgerPocketIcClient;

use crate::common::TestContext;

mod cycles_ledger;
mod icp_cli;
mod icp_ledger;

pub fn icp(ctx: &TestContext, current_dir: impl Into<PathBuf>) -> IcpCliClient<'_> {
    IcpCliClient::new(ctx, current_dir.into())
}

pub fn icp_ledger(ctx: &TestContext) -> IcpLedgerPocketIcClient<'_> {
    IcpLedgerPocketIcClient::new(ctx)
}

pub fn cycles_ledger(ctx: &TestContext) -> CyclesLedgerPocketIcClient<'_> {
    CyclesLedgerPocketIcClient::new(ctx)
}
