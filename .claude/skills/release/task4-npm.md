# Task 4: Publish to NPM

*Requires Task 3 to be complete.*

```bash
# "Publish to npm" is the workflow *name* (not filename release-npm.yml)
gh workflow run "Publish to npm" \
  --field version=v$ARGUMENTS \
  --field npm_package_version=$ARGUMENTS \
  --field beta=$([[ "$ARGUMENTS" =~ -beta\. ]] && echo true || echo false)
```

Wait for GitHub to register the run, then find the run triggered after the dispatch:
```bash
sleep 10
NPM_RUN_ID=$(gh run list --workflow "Publish to npm" --limit 1 --json databaseId,status \
  --jq '[.[] | select(.status != "completed")] | .[0].databaseId // empty')
if [ -z "$NPM_RUN_ID" ]; then
  # All runs completed — just grab the latest one
  NPM_RUN_ID=$(gh run list --workflow "Publish to npm" --limit 1 --json databaseId --jq '.[0].databaseId')
fi
NPM_RUN_URL="https://github.com/dfinity/icp-cli/actions/runs/${NPM_RUN_ID}"
echo "Watching: ${NPM_RUN_URL}"
gh run watch ${NPM_RUN_ID} --exit-status
```

If it succeeds, notify the release driver: "NPM publish completed for v$ARGUMENTS."

If it fails, notify the release driver: "NPM publish failed for v$ARGUMENTS: ${NPM_RUN_URL} — please investigate."
