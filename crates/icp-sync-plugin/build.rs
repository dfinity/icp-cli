use camino::Utf8PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=sync-plugin.wit");
    println!("cargo:rerun-if-changed=tests/fixtures/test-plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/test-plugin/Cargo.toml");

    if wasm32_wasip2_is_installed() {
        build_test_fixture();
    }
}

fn wasm32_wasip2_is_installed() -> bool {
    let Ok(output) = Command::new("rustc").args(["--print", "sysroot"]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let sysroot = String::from_utf8_lossy(&output.stdout);
    Utf8PathBuf::from(sysroot.trim())
        .join("lib/rustlib/wasm32-wasip2")
        .exists()
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
        .status()
        .expect("failed to spawn cargo build for test fixture");
    assert!(
        status.success(),
        "cargo build --target wasm32-wasip2 failed for test fixture"
    );
    let wasm = fixture_target_dir.join("wasm32-wasip2/release/test_plugin.wasm");
    println!("cargo:rustc-env=TEST_PLUGIN_WASM={wasm}");
}
