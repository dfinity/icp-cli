# Task 5: Publish to dfinity/homebrew-tap

*Requires Task 3 to be complete (needs release binaries). Runs concurrently with Task 4.*

**1. Trigger the update workflow**

The `update-icp-cli-beta.yml` workflow in `dfinity/homebrew-tap` handles formula updates and PR creation. The version input must be **without** the `v` prefix:
```bash
gh workflow run update-icp-cli-beta.yml --repo dfinity/homebrew-tap \
  --field version=$ARGUMENTS
```

**2. Find and watch the workflow run**
```bash
sleep 10
TAP_RUN_ID=$(gh run list --repo dfinity/homebrew-tap --workflow update-icp-cli-beta.yml --limit 1 \
  --json databaseId --jq '.[0].databaseId')
if [ -z "$TAP_RUN_ID" ]; then
  echo "ERROR: Could not find homebrew-tap workflow run"
  exit 1
fi
TAP_RUN_URL="https://github.com/dfinity/homebrew-tap/actions/runs/${TAP_RUN_ID}"
echo "Watching: ${TAP_RUN_URL}"
gh run watch --repo dfinity/homebrew-tap ${TAP_RUN_ID} --exit-status
```

If it fails, notify the release driver: "homebrew-tap workflow failed for $ARGUMENTS: ${TAP_RUN_URL} — please investigate."

**3. Watch the generated PR until merge**

The workflow creates a PR titled `icp-cli-beta $ARGUMENTS` with the `merge-without-publishing` label. Find the PR and watch its status:
```bash
TAP_PR_URL=$(gh pr list --repo dfinity/homebrew-tap \
  --search "icp-cli-beta $ARGUMENTS" --json url --jq '.[0].url')
echo "homebrew-tap PR: ${TAP_PR_URL}"
```

Poll until the PR is merged:
```bash
TAP_PR_STATE=$(gh pr view --repo dfinity/homebrew-tap \
  "update/icp-cli-beta-$ARGUMENTS" --json state --jq '.state')
echo "PR state: ${TAP_PR_STATE}"
```

Once `TAP_PR_STATE` is `MERGED`, notify the release driver: "homebrew-tap PR merged: ${TAP_PR_URL}"

If the PR has failing checks or is not progressing, notify the release driver: "homebrew-tap PR needs attention: ${TAP_PR_URL}"
