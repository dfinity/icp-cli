# icp-cli

## Running Tests

These tests use dfx to stand up and interact with a local Internet Computer instance.
To ensure test isolation, they run in a temporary HOME directory and 
**cannot use the dfx shim from dfxvm**.

To run the tests:

```
# Ensure dfx is installed
dfx cache install

# Export the path to the actual dfx binary (not the shim)
export ICPTEST_DFX_PATH="$(dfx cache show)/dfx"

# Run tests (may include #[ignore] tests depending on setup)
cargo test
```

If ICPTEST_DFX_PATH is not set, tests that depend on dfx will be skipped or marked as ignored.

