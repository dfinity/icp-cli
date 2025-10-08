use crate::common::TestContext;

mod common;

#[tokio::test]
async fn main_version_gets_printed() {
    let ctx = TestContext::new();
    // does not exit properly if the version string is not defined
    ctx.icp().args(["--version"]).assert().success();
}
