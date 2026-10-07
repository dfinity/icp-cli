import os
import json
import re

TEST_DIR = "crates/icp-cli/tests"

MACOS_TESTS = ["network_tests"]

# On Windows a local network runs in Docker (inside WSL2), which is slow and
# flaky to set up, so only test files that start a network get it. A file this
# misses fails loudly on Windows rather than skipping anything.
NEEDS_NETWORK = re.compile(r'start_network|ping_until_healthy|docker|"network",\s*"start"')


def needs_network(test):
    with open(os.path.join(TEST_DIR, f"{test}.rs")) as f:
        return NEEDS_NETWORK.search(f.read()) is not None


def test_names():
    all_files = os.listdir(TEST_DIR)
    rust_files = filter(lambda f: f.endswith(".rs"), all_files)
    return [f"{filename[:-3]}" for filename in rust_files]


include = []
for test in test_names():
    # Ubuntu/Windows: run everything
    include.append({
        "test": test,
        "os": "ubuntu-22.04"
    })
    include.append({
        "test": test,
        "os": "windows-2025",
        "docker": needs_network(test)
    })

    # macOS: only run selected tests
    if test in MACOS_TESTS:
        include.append({
            "test": test,
            "os": "macos-15"
        })


matrix = {
    "include": include,
}

print(json.dumps(matrix))
