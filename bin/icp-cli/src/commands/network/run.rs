use crate::project::structure::ProjectStructure;
use clap::Parser;
use icp_network::structure::NetworkDirectoryStructure;
use icp_network::{ManagedNetworkModel, run_local_network};

#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(cmd: Cmd) {
    println!("Running network command");

    let config = ManagedNetworkModel::default();
    let ps = ProjectStructure::find().unwrap();
    eprintln!("Project structure root: {}", ps.root().display());
    let network_root = ps.network_root("local");
    eprintln!("Network root: {}", network_root.display());

    let nds = NetworkDirectoryStructure::new(&network_root);
    run_local_network(config, nds).await.unwrap();
}
