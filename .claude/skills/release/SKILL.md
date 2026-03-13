---
name: release
description: Release a new version of icp-cli. Use when the user asks to do a release or cut a new version. Requires a semver VERSION argument.
argument-hint: <VERSION>
disable-model-invocation: true
allowed-tools: Read, Edit, Bash(git *), Bash(gh *), Bash(cargo check -q), Bash(curl *), Bash(jq *), Bash(shasum *), Bash(awk *), Bash(sed *), Bash(base64 *), Bash(tr *), Bash(uname *), Bash(sleep *), Bash(echo *)
---

Release VERSION: **$ARGUMENTS**

**Prerequisites check** — verify before proceeding:
```bash
gh --version   # if missing: https://cli.github.com
git --version
```
If either is missing, stop and ask the user to install it.

**Validate VERSION and determine release mode:**
```bash
VERSION="$ARGUMENTS"
if [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  RELEASE_MODE=stable
elif [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$ ]]; then
  RELEASE_MODE=beta
else
  echo "ERROR: '$VERSION' is not a valid version. Must be X.Y.Z or X.Y.Z-beta.N (e.g. 0.2.0 or 0.2.0-beta.0)."
  exit 1
fi
echo "${VERSION} is valid. Starting a ${RELEASE_MODE} release."
```
If validation fails, stop and inform the user.

## Dependency order

```
Task 1 (PR)
    |
Task 2 (tag)
    |
    +-------------------+-------------------+
    |                   |                   |
Task 3              Task 5              Task 7
(Release workflow)  (homebrew-tap)      (docs site versions)
    |               [beta only]         [stable only]
Task 4 (NPM)
    |
Task 6 (homebrew-core check)
[stable only]
```

Task 5 starts immediately after the tag is pushed and runs concurrently with Tasks 3 & 4. Task 4 requires Task 3 to complete first (needs GitHub release artifacts). Task 6 runs after Task 4 and is only for stable releases. Task 7 starts immediately after the tag is pushed (concurrently with Tasks 3 & 4) and is only for stable releases; it must wait for the docs deployment triggered by the tag before its PR can be merged.

---

## Task 1: Bump the version and open a release PR

**0. Branch**
```bash
git checkout main && git pull origin main
USERNAME=$(gh api user --jq '.login')
git checkout -b ${USERNAME}/release_$ARGUMENTS
```

**1. Bump version** — edit `[workspace.package] version` in `Cargo.toml`, then:
```bash
cargo check -q   # updates Cargo.lock
```

**2. Update `CHANGELOG.md`**

Structure: `# Unreleased` (always empty) → `# v<VERSION>` → older versions.

If `$ARGUMENTS` is a stable release and a beta header `# v$ARGUMENTS-beta.N` exists:
- Remove that beta header
- Prepend any `# Unreleased` entries to the beta's bullet list
- Replace beta header with `# v$ARGUMENTS`
- Leave `# Unreleased` empty

**3. Commit**
```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: release v$ARGUMENTS"
```

**4. Draft PR**
```bash
git push -u origin "$(gh api user --jq '.login')/release_$ARGUMENTS"
gh pr create --draft \
  --title "chore: release v$ARGUMENTS" \
  --body "$(cat <<'EOF'
## Summary

- `Cargo.toml`: version bumped to `$ARGUMENTS`
- `Cargo.lock`: updated
- `CHANGELOG.md`: entries consolidated under `# v$ARGUMENTS`

### Review checklist
- [ ] CI passes
- [ ] Changelog entries look correct
- [ ] Version number is correct
EOF
)"
```

**5. Monitor CI and notify**
```bash
gh pr checks --watch
```

If all checks pass:
```bash
gh pr ready
PR_URL=$(gh pr view --json url --jq '.url')
```
Notify the release driver: "PR is ready for review: ${PR_URL}"

If any check fails:
```bash
PR_URL=$(gh pr view --json url --jq '.url')
```
Notify the release driver: "PR has failing CI: ${PR_URL} — please fix or rerun flaky tests."

---

## Task 2: Tag

Wait for the release PR to be approved and merged. Poll until the PR is merged:
```bash
PR_STATE=$(gh pr view --json state --jq '.state')
echo "PR state: ${PR_STATE}"
```
If `PR_STATE` is `OPEN`, notify the release driver: "Waiting for PR to be approved and merged. Let me know when it's merged, or I can check again." Do **not** proceed until the PR state is `MERGED`.

Once merged:
```bash
git checkout main && git pull origin main
git tag v$ARGUMENTS
git push origin v$ARGUMENTS
```

**After the tag is pushed, start Task 3 in background. If `$ARGUMENTS` is a beta release, also start Task 5 concurrently in background. If `$ARGUMENTS` is a stable release, also start Task 7 concurrently in background.**

---

## Task 3: Monitor Release workflow

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

If it succeeds, proceed to Task 4.

If it fails, notify the release driver: "Release workflow failed for v$ARGUMENTS: ${RELEASE_RUN_URL} — please investigate before proceeding."

---

## Task 4: Publish to NPM

*Requires Task 3 to be complete.*

```bash
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

---

## Task 5: Publish to dfinity/homebrew-tap (beta releases only)

*Skip if `$ARGUMENTS` is a stable release. Requires Task 2. Runs concurrently with Tasks 3 & 4.*

Formula: `Formula/icp-cli-beta.rb` in `dfinity/homebrew-tap`. Only `url` (line 4) and the top-level `sha256` (line 5) need updating — leave the `bottle` block alone, CI regenerates it.

**1. Compute SHA256, create branch, and update formula**
```bash
BRANCH="bump-icp-cli-beta-$ARGUMENTS"

# Compute tarball SHA256 (--fail ensures we don't hash an error page)
NEW_SHA=$(curl -sfL "https://github.com/dfinity/icp-cli/archive/refs/tags/v$ARGUMENTS.tar.gz" \
  | shasum -a 256 | awk '{print $1}')
if [ -z "$NEW_SHA" ]; then
  echo "ERROR: Failed to download tarball for v$ARGUMENTS — tag may not exist yet on GitHub"
  exit 1
fi

# Create branch on dfinity/homebrew-tap
BASE=$(gh api repos/dfinity/homebrew-tap/git/ref/heads/main --jq '.object.sha')
gh api repos/dfinity/homebrew-tap/git/refs \
  -f ref="refs/heads/${BRANCH}" -f sha="${BASE}"

# Update the formula file via API
BLOB_SHA=$(gh api repos/dfinity/homebrew-tap/contents/Formula/icp-cli-beta.rb --jq '.sha')
OLD=$(gh api repos/dfinity/homebrew-tap/contents/Formula/icp-cli-beta.rb --jq '.content' \
  | { case "$(uname)" in Darwin) base64 -D;; *) base64 -d;; esac; })
NEW=$(echo "$OLD" \
  | sed "4s|refs/tags/v[^\"]*\.tar\.gz|refs/tags/v$ARGUMENTS.tar.gz|" \
  | sed "5s/\"[a-f0-9]*\"/\"${NEW_SHA}\"/")
NEW_B64=$(echo "$NEW" | base64 | tr -d '\n')
gh api repos/dfinity/homebrew-tap/contents/Formula/icp-cli-beta.rb \
  -X PUT \
  -f message="icp-cli-beta $ARGUMENTS" \
  -f content="${NEW_B64}" \
  -f sha="${BLOB_SHA}" \
  -f branch="${BRANCH}"
```

**2. Open a draft PR**
```bash
gh pr create --repo dfinity/homebrew-tap \
  --head "bump-icp-cli-beta-$ARGUMENTS" \
  --draft \
  --title "icp-cli-beta $ARGUMENTS" \
  --body "Bump icp-cli-beta to $ARGUMENTS"
```

**3. After PR is created**

`confirm-publish:required` will fail — that's expected. If any other check fails, notify the driver to investigate before proceeding.

Add the `!add-bottles` label:
```bash
gh pr edit --repo dfinity/homebrew-tap --add-label '!add-bottles' "bump-icp-cli-beta-$ARGUMENTS"
```

**4. After `github-actions` bot commits bottle hashes**

Wait for the bot to push bottle SHA256s. The `confirm-publish:required` check will fail — that is expected and does not mean the process has failed. Poll for the bot commit by checking the PR commit count:
```bash
INITIAL_COMMITS=$(gh pr view --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --json commits --jq '.commits | length')
echo "Initial commits: ${INITIAL_COMMITS}. Waiting for bot to add bottle hashes..."
```

Poll every 30 seconds until the commit count increases (the bot has pushed):
```bash
CURRENT_COMMITS=$(gh pr view --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --json commits --jq '.commits | length')
echo "Current commits: ${CURRENT_COMMITS}"
```
Once `CURRENT_COMMITS` > `INITIAL_COMMITS`, the bot has committed.

`External PR Ruleset` will be stuck (bot commit doesn't trigger it). Close and reopen to retrigger:
```bash
gh pr close --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS"
gh pr reopen --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS"
```

Monitor checks (ignore `confirm-publish:required` failures — they are expected):
```bash
gh pr checks --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --watch
```

If any check **other than `confirm-publish:required`** fails, stop and notify the driver to investigate.

If all other checks pass, proceed to Step 5.

**5. Convert to ready for review and notify**
```bash
gh pr ready --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS"
TAP_PR_URL=$(gh pr view --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --json url --jq '.url')
```
Notify the release driver: "homebrew-tap PR is ready for review: ${TAP_PR_URL}"

---

## Task 6: Check homebrew-core status (stable releases only)

*Skip if `$ARGUMENTS` is a beta release. Requires Task 4 to be complete.*

Check the homebrew-core PR and extract its URL and state:
```bash
HBC_PR=$(gh pr list --repo Homebrew/homebrew-core \
  --search "icp-cli $ARGUMENTS" \
  --json number,state,url,mergedAt \
  --state all)
HBC_PR_URL=$(echo "$HBC_PR" | jq -r '.[0].url // ""')
HBC_PR_STATE=$(echo "$HBC_PR" | jq -r '.[0].state // ""')
```

Determine the **homebrew status line** to use in the release announcement:

- If `$HBC_PR_URL` is empty (no PR found):
  `- Homebrew: stable release will be published to homebrew-core — BrewTestBot hasn't created the PR yet, check https://github.com/Homebrew/homebrew-core/pulls?q=is%3Apr+icp-cli+$ARGUMENTS later`
- If `$HBC_PR_STATE` is `OPEN`:
  `- Homebrew: stable release will be published to homebrew-core — formula PR is in review: $HBC_PR_URL`
- If `$HBC_PR_STATE` is `MERGED`: check whether the new version is live:
  ```bash
  curl -sf https://formulae.brew.sh/api/formula/icp-cli.json | jq -r '.versions.stable'
  ```
  - If the returned version equals `$ARGUMENTS`:
    `- Homebrew: stable release has been published to homebrew-core. \`brew install icp-cli\` (or \`brew upgrade icp-cli\`)`
  - If the returned version does not equal `$ARGUMENTS`:
    `- Homebrew: stable release will be published to homebrew-core — formula PR merged but not yet propagated: $HBC_PR_URL`

Proceed to the release announcement with the homebrew status line determined above.

---

## Task 7: Update docs site versions (stable releases only)

*Skip if `$ARGUMENTS` is a beta release. Requires Task 2. Runs concurrently with Tasks 3 & 4.*

The tag push triggers a docs deployment workflow that builds and publishes the versioned docs to `/icp-cli/X.Y/`. The `versions.json` PR must not be merged until that deployment succeeds, otherwise the root redirect will point to a path that does not exist yet.

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
gh pr create --draft \
  --title "chore: update docs site to v${MINOR_VERSION}" \
  --body "$(cat <<'EOF'
## Summary

- `docs-site/versions.json`: add v${MINOR_VERSION} as the new latest version

Updates the version switcher and root redirect (`dfinity.github.io/icp-cli/`) to point to the new stable release. Must be merged only after the versioned docs are confirmed deployed.
EOF
)"
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

---

## Release announcement

When all tasks are complete, output a message ready to copy to the team channel.

If `$ARGUMENTS` is a stable release, output (using the homebrew status line from Task 6):
```
🚀 icp-cli v$ARGUMENTS released!
- Release: https://github.com/dfinity/icp-cli/releases/tag/v$ARGUMENTS
- NPM: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$ARGUMENTS
- <homebrew status line from Task 6>
```

If `$ARGUMENTS` is a beta release, output:
```
🚀 icp-cli v$ARGUMENTS released!
- Release: https://github.com/dfinity/icp-cli/releases/tag/v$ARGUMENTS
- NPM: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$ARGUMENTS
- Homebrew: beta release has been published to dfinity/homebrew-tap.
    `brew install dfinity/tap/icp-cli-beta`
```

---

## Rollback guidance

If something fails mid-release, here's how to clean up depending on how far you got:

- **Task 1 failed (PR)**: Close the PR and delete the branch. No external state was changed.
- **Task 2 failed (tag pushed but workflows fail)**: The tag can be deleted if the Release workflow hasn't created a GitHub Release yet:
  ```bash
  git push origin --delete v$ARGUMENTS
  git tag -d v$ARGUMENTS
  ```
  If a GitHub Release was already created, delete it first via `gh release delete v$ARGUMENTS --yes`, then delete the tag.
- **Task 3 failed (Release workflow)**: Investigate the failure. The tag still exists. Once fixed, you can re-run the workflow from the GitHub Actions UI. Do **not** delete and re-push the tag — that creates duplicate runs.
- **Task 4 failed (NPM publish)**: NPM publishes are not easily reversible. If the publish partially succeeded, check `npm info @icp-sdk/icp-cli versions` and coordinate with the team. The workflow can be re-triggered from the GitHub Actions UI.
- **Task 5 failed (homebrew-tap)**: Close the PR on `dfinity/homebrew-tap` and delete the branch via the GitHub UI. No packages were published.
- **Task 7 failed (docs versions)**: Close the PR and delete the branch. The versioned docs are deployed independently and are unaffected.
