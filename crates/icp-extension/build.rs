use camino::Utf8PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=extension.wit");
    println!("cargo:rerun-if-changed=tests/fixtures/hello-extension/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/hello-extension/Cargo.toml");

    if wasm32_wasip2_is_installed() {
        build_test_fixture("hello-extension", "hello_extension", "HELLO_EXTENSION_WASM");
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

fn build_test_fixture(crate_dir: &str, wasm_stem: &str, env_var: &str) {
    let manifest_dir = Utf8PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = Utf8PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let fixture_manifest = manifest_dir.join(format!("tests/fixtures/{crate_dir}/Cargo.toml"));
    let fixture_target_dir = out_dir.join(format!("fixture-target/{crate_dir}"));
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
        "cargo build --target wasm32-wasip2 failed for test fixture {crate_dir}"
    );
    let wasm = fixture_target_dir.join(format!("wasm32-wasip2/release/{wasm_stem}.wasm"));
    println!("cargo:rustc-env={env_var}={wasm}");
}
