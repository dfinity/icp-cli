import os
import json

TEST_DIR = "crates/icp-cli/tests"

MACOS_TESTS = ["network_tests"]

# Test files whose tests need a Docker daemon (Docker-backed managed networks).
# On Windows the daemon runs in WSL2, and that setup (setup-wsl + dockerd) adds
# ~1 min per job, so only these jobs run it. Linux uses its preinstalled Docker,
# so the flag is a no-op there. Keep in sync with the tests that use Docker.
DOCKER_TESTS = [
    "bundle_tests",
    "canister_create_tests",
    "deploy_tests",
    "network_tests",
]


def test_names():
    all_files = os.listdir(TEST_DIR)
    rust_files = filter(lambda f: f.endswith(".rs"), all_files)
    return [f"{filename[:-3]}" for filename in rust_files]


include = []
for test in test_names():
    needs_docker = test in DOCKER_TESTS
    # Ubuntu/Windows: run everything
    include.append({
        "test": test,
        "os": "ubuntu-22.04",
        "needs_docker": needs_docker,
    })
    include.append({
        "test": test,
        "os": "windows-2025",
        "needs_docker": needs_docker,
    })

    # macOS: only run selected tests
    if test in MACOS_TESTS:
        include.append({
            "test": test,
            "os": "macos-15",
            "needs_docker": needs_docker,
        })


matrix = {
    "include": include,
}

print(json.dumps(matrix))
