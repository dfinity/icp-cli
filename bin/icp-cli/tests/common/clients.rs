use cycles_ledger::CyclesLedgerPocketIcClient;
use icp_cli::IcpCliClient;
use icp_ledger::IcpLedgerPocketIcClient;

use crate::common::TestContext;

mod cycles_ledger;
mod icp_cli;
mod icp_ledger;

pub fn icp_client(ctx: &TestContext) -> IcpCliClient<'_> {
    IcpCliClient::new(ctx)
}

pub fn icp_ledger(ctx: &TestContext) -> IcpLedgerPocketIcClient<'_> {
    IcpLedgerPocketIcClient::new(ctx)
}

pub fn cycles_ledger(ctx: &TestContext) -> CyclesLedgerPocketIcClient<'_> {
    CyclesLedgerPocketIcClient::new(ctx)
}
