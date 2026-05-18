# Task 3: Monitor Release workflow

The Release workflow is triggered by the tag push. Find the run that matches the tag:
```bash
sleep 10
RELEASE_RUN_ID=$(gh run list --workflow release.yml --branch "v$ARGUMENTS" --limit 1 \
  --json databaseId --jq '.[0].databaseId')
if [ -z "$RELEASE_RUN_ID" ]; then
  echo "ERROR: Could not find Release workflow run for tag v$ARGUMENTS"
  exit 1
fi
RELEASE_RUN_URL="https://github.com/dfinity/icp-cli/actions/runs/${RELEASE_RUN_ID}"
echo "Watching: ${RELEASE_RUN_URL}"
gh run watch ${RELEASE_RUN_ID} --exit-status
```

If it succeeds, start Tasks 4 and 5 concurrently.

If it fails, notify the release driver: "Release workflow failed for v$ARGUMENTS: ${RELEASE_RUN_URL} — please investigate before proceeding."
