use camino::Utf8PathBuf;
use cycles_ledger::CyclesLedgerPocketIcClient;
pub use icp_cli::IcpCliClient;
use icp_ledger::IcpLedgerPocketIcClient;

use crate::common::TestContext;

mod cycles_ledger;
mod icp_cli;
mod icp_ledger;

pub fn icp<'a>(ctx: &'a TestContext, current_dir: &'a Utf8PathBuf) -> IcpCliClient<'a> {
    IcpCliClient::new(ctx, current_dir)
}

pub fn icp_ledger(ctx: &TestContext) -> IcpLedgerPocketIcClient<'_> {
    IcpLedgerPocketIcClient::new(ctx)
}

pub fn cycles_ledger(ctx: &TestContext) -> CyclesLedgerPocketIcClient<'_> {
    CyclesLedgerPocketIcClient::new(ctx)
}
