# icp-cli

## Running the local network

The `ICP_POCKET_IC_PATH` environment variable should point to
the path of the `pocket-ic` binary.

## Running Tests

These tests use dfx to stand up and interact with a local Internet Computer instance.
To ensure test isolation, they run in a temporary HOME directory and 
**cannot use the dfx shim from dfxvm**.

To run the tests, it's necessary to set the ICPTEST_DFX_PATH environment variable
to a valid dfx path. Here is one way to do this:

```
# Ensure dfx is installed and the cache is populated
dfx cache install

# Export the path to the actual dfx binary (not the shim)
export ICPTEST_DFX_PATH="$(dfx cache show)/dfx"

# Export the path to the pocket-ic binary
export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"

# Run tests
cargo test
```

If ICPTEST_DFX_PATH is not set, tests that depend on dfx will fail.
