import os
import json

TEST_DIR = "bin/icp-cli/tests"

MACOS_TESTS = ["network_tests"]


def test_names():
    all_files = os.listdir(TEST_DIR)
    rust_files = filter(lambda f: f.endswith(".rs"), all_files)
    return [f"{filename[:-3]}" for filename in rust_files]


include = []
for test in test_names():
    # Ubuntu: run everything
    include.append({
        "test": test,
        "os": "ubuntu-24.04"
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
