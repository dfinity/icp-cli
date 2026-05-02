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

    // Building the wasm test fixture requires the `wasm32-wasip2` target to be
    // installed. Distribution toolchains (Homebrew, distro packagers — see
    // dfinity/icp-cli#543) do not always include it, and the fixture is only
    // referenced by `#[test]` functions in `runtime.rs`. Emit a warning and
    // leave `TEST_PLUGIN_WASM` empty rather than panicking the whole build;
    // any test that actually depends on the fixture will surface a clearer
    // "fixture not built" error when the empty path is used.
    let succeeded = matches!(&status, Ok(s) if s.success());
    if !succeeded {
        match status {
            Err(e) => println!(
                "cargo:warning=icp-sync-plugin: failed to spawn `cargo build --target wasm32-wasip2` for the test fixture: {e}; tests requiring TEST_PLUGIN_WASM will fail."
            ),
            Ok(_) => println!(
                "cargo:warning=icp-sync-plugin: `cargo build --target wasm32-wasip2` failed for the test fixture (target probably not installed); tests requiring TEST_PLUGIN_WASM will fail."
            ),
        }
        println!("cargo:rustc-env=TEST_PLUGIN_WASM=");
        return;
    }

    let wasm = fixture_target_dir.join("wasm32-wasip2/release/test_plugin.wasm");
    println!("cargo:rustc-env=TEST_PLUGIN_WASM={wasm}");
}
