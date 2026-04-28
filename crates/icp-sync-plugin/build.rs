use camino::Utf8PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=sync-plugin.wit");
    println!("cargo:rerun-if-changed=tests/fixtures/test-plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/test-plugin/Cargo.toml");

    build_test_fixture();
}

fn build_test_fixture() {
    let manifest_dir = Utf8PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = Utf8PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let fixture_manifest = manifest_dir.join("tests/fixtures/test-plugin/Cargo.toml");
    let fixture_target_dir = out_dir.join("fixture-target");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let status = Command::new(&cargo)
        .args([
            "build",
            "--target",
            "wasm32-wasip2",
            "--release",
            "--locked",
            "--manifest-path",
            fixture_manifest.as_str(),
            "--target-dir",
            fixture_target_dir.as_str(),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            let wasm = fixture_target_dir.join("wasm32-wasip2/release/test_plugin.wasm");
            println!("cargo:rustc-env=TEST_PLUGIN_WASM={wasm}");
        }
        _ => {
            // wasm32-wasip2 target not installed or build failed; fixture-dependent
            // tests will be skipped via option_env!("TEST_PLUGIN_WASM").
        }
    }
}
