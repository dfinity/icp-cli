mod common;

use crate::common::TestEnv;
use predicates::str::contains;

#[test]
fn hello() {
    let testenv = TestEnv::new();
    testenv
        .icp()
        .arg("network")
        .arg("run")
        .assert()
        .success()
        .stdout(contains("Hello, world!"));
}
