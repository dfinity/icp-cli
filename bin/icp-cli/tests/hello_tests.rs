mod common;

use crate::common::TestEnv;
use predicates::str::contains;

#[test]
fn hello() {
    let testenv = TestEnv::new();
    testenv
        .icp()
        .assert()
        .success()
        .stdout(contains("Hello, world!"));
}
