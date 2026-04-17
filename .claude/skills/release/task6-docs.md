# Task 6: Update docs site versions (stable releases only)

*Skip if `$ARGUMENTS` is a beta release. Requires Task 2. Runs concurrently with Task 3.*

The tag push triggers two automated workflows:

1. **`docs.yml` (`publish-versioned-docs` job):** Builds and publishes the versioned docs to `/X.Y/` on the `docs-deployment` branch (served at `https://cli.internetcomputer.org/X.Y/`). The `versions.json` PR must not be merged until that deployment succeeds, otherwise the root redirect will point to a path that does not exist yet.

2. **`sync-docs-branch.yml`:** Opens a PR to reset `docs/vX.Y` to the new tag, preventing the branch from drifting behind the latest patch release. **Merge this PR (squash) after CI passes.** If there are docs-only improvements on `main` that should be backported to this version's docs, cherry-pick them onto `docs/vX.Y` after merging.

Once the `versions.json` PR merges to `main`, the `publish-root-files` CI job runs automatically and copies `og-image.png`, `llms.txt`, `llms-full.txt`, and `feed.xml` from the new version's folder to the deployment root — no manual step needed.

**1. Wait for the docs deployment triggered by the tag**
```bash
sleep 10
DOCS_RUN_ID=$(gh run list --workflow docs.yml --branch "v$ARGUMENTS" --limit 1 \
  --json databaseId --jq '.[0].databaseId')
if [ -z "$DOCS_RUN_ID" ]; then
  echo "ERROR: Could not find docs workflow run for tag v$ARGUMENTS"
  exit 1
fi
DOCS_RUN_URL="https://github.com/dfinity/icp-cli/actions/runs/${DOCS_RUN_ID}"
echo "Watching docs deploy: ${DOCS_RUN_URL}"
gh run watch ${DOCS_RUN_ID} --exit-status
```

If it fails, notify the release driver: "Docs deployment failed for v$ARGUMENTS: ${DOCS_RUN_URL} — please investigate before merging the versions.json PR."

**2. Branch and update `docs-site/versions.json`**
```bash
MINOR_VERSION=$(echo "$ARGUMENTS" | sed 's/\.[0-9]*$//')
git checkout main && git pull origin main
USERNAME=$(gh api user --jq '.login')
git checkout -b ${USERNAME}/docs-versions-$ARGUMENTS

jq --arg v "$MINOR_VERSION" \
  '.versions = [{version: $v, latest: true}] + (.versions | map(del(.latest)))' \
  docs-site/versions.json > docs-site/versions.json.tmp && mv docs-site/versions.json.tmp docs-site/versions.json
```

**3. Commit and draft PR**
```bash
git add docs-site/versions.json
git commit -m "chore: update docs site to v${MINOR_VERSION}"
git push -u origin ${USERNAME}/docs-versions-$ARGUMENTS
cat > /tmp/docs-pr-body.md <<EOF
## Summary

- \`docs-site/versions.json\`: add v${MINOR_VERSION} as the new latest version

Updates the version switcher and root redirect (\`dfinity.github.io/icp-cli/\`) to point to the new stable release. Must be merged only after the versioned docs are confirmed deployed.
EOF
gh pr create --draft \
  --title "chore: update docs site to v${MINOR_VERSION}" \
  --body-file /tmp/docs-pr-body.md
```

**4. Monitor CI and notify**
```bash
gh pr checks --watch
```

If all checks pass:
```bash
gh pr ready
DOCS_PR_URL=$(gh pr view --json url --jq '.url')
```
Notify the release driver: "Docs versions PR is ready for review: ${DOCS_PR_URL}"

If any check fails:
```bash
DOCS_PR_URL=$(gh pr view --json url --jq '.url')
```
Notify the release driver: "Docs versions PR has failing CI: ${DOCS_PR_URL} — please fix or rerun flaky tests."
