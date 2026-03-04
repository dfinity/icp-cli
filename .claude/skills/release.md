---
name: release
description: Release a new version of icp-cli. Use when the user asks to do a release or cut a new version. Requires a semver VERSION argument.
argument-hint: <VERSION>
disable-model-invocation: true
allowed-tools: Read, Edit, Glob, Grep, Bash(git *), Bash(gh *), Bash(cargo check -q), Bash(curl *), Bash(shasum *), Bash(awk *), Bash(sed *), Bash(base64 *), Bash(tr *), Bash(uname *), Bash(sleep *), Bash(echo *)
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
                                            !IS_BETA -> Part 5a (homebrew-core)
                                           /
Part 1 (PR) -> Part 2 (tag) --+-- [IS_BETA?]
                               |           \
                               |            IS_BETA -> Part 5b (homebrew-tap)
                               |
                               +-- Part 3 (Release workflow) -> Part 4 (npm)
```

Parts 3, 5a, and 5b all start immediately after the tag is pushed. Parts 5a/5b do **not** wait for the Release workflow. Part 4 (npm) requires Part 3 to complete first (needs GitHub release artifacts).

---

## Part 1: icp-cli repo

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

## Part 2: Tag

Wait for the release PR to be approved and merged, then:

```bash
git checkout main && git pull origin main
git tag v$ARGUMENTS
git push origin v$ARGUMENTS
```

**After the tag is pushed, start Parts 3, 5a, and 5b concurrently in background.**

---

## Part 3: Monitor Release workflow

```bash
sleep 5
RELEASE_RUN_ID=$(gh run list --limit 5 --json databaseId,workflowName \
  --jq '[.[] | select(.workflowName == "Release")] | .[0].databaseId')
RELEASE_RUN_URL="https://github.com/dfinity/icp-cli/actions/runs/${RELEASE_RUN_ID}"
echo "Watching: ${RELEASE_RUN_URL}"
gh run watch ${RELEASE_RUN_ID} --exit-status
```

If it succeeds, proceed to Part 4.

If it fails, notify the release driver: "Release workflow failed for v$ARGUMENTS: ${RELEASE_RUN_URL} — please investigate before proceeding."

---

## Part 4: Publish to npm

*Requires Part 3 to be complete.*

```bash
gh workflow run "Publish to npm" \
  --field version=v$ARGUMENTS \
  --field npm_package_version=$ARGUMENTS \
  --field beta=$([[ "$ARGUMENTS" =~ -beta\. ]] && echo true || echo false)
```

Wait a moment for GitHub to register the run, then get its ID and watch it:
```bash
sleep 5
NPM_RUN_ID=$(gh run list --workflow "Publish to npm" --limit 1 --json databaseId --jq '.[0].databaseId')
NPM_RUN_URL="https://github.com/dfinity/icp-cli/actions/runs/${NPM_RUN_ID}"
echo "Watching: ${NPM_RUN_URL}"
gh run watch ${NPM_RUN_ID} --exit-status
```

If it succeeds, notify the release driver: "npm publish completed for v$ARGUMENTS."

If it fails, notify the release driver: "npm publish failed for v$ARGUMENTS: ${NPM_RUN_URL} — please investigate."

---

## Part 5a: Publish to homebrew-core (stable releases only)

*Requires Part 2. Runs concurrently with Parts 3 & 4. Skip if `$ARGUMENTS` is a beta release.*

No action required. Notify the release driver:

> "This is a stable release. BrewTestBot will automatically bump the `icp-cli` formula in homebrew-core — no action needed from our side. The full process (bot PR + CI + maintainer review) may take several hours. Please check https://formulae.brew.sh/formula/icp-cli later to confirm it reflects v$ARGUMENTS."

---

## Part 5b: Publish to dfinity/homebrew-tap (beta releases only)

*Requires Part 2. Runs concurrently with Parts 3 & 4. Skip if `$ARGUMENTS` is a stable release.*

Formula: `Formula/icp-cli-beta.rb` in `dfinity/homebrew-tap`. Only `url` (line 4) and the top-level `sha256` (line 5) need updating — leave the `bottle` block alone, CI regenerates it.

**1. Compute SHA256, create branch, and update formula**
```bash
BRANCH="bump-icp-cli-beta-$ARGUMENTS"

# Compute tarball SHA256
NEW_SHA=$(curl -sL "https://github.com/dfinity/icp-cli/archive/refs/tags/v$ARGUMENTS.tar.gz" \
  | shasum -a 256 | awk '{print $1}')

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

Wait for the bot to push bottle SHA256s and initial checks to settle:
```bash
gh pr checks --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --watch
```

`External PR Ruleset` will be stuck (bot commit doesn't trigger it). Close and reopen to retrigger:
```bash
gh pr close --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS"
gh pr reopen --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS"
```

Monitor checks:
```bash
gh pr checks --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --watch
```

If any check fails, stop and notify the driver to investigate.

If all checks pass, proceed to Step 5.

**5. Convert to ready for review and notify**
```bash
gh pr ready --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS"
TAP_PR_URL=$(gh pr view --repo dfinity/homebrew-tap "bump-icp-cli-beta-$ARGUMENTS" --json url --jq '.url')
```
Notify the release driver: "homebrew-tap PR is ready for review: ${TAP_PR_URL}"

---

## Release announcement

When all parts are complete, output a message ready to copy to the team channel.

If `$ARGUMENTS` is a stable release, output:
```
🚀 icp-cli v$ARGUMENTS released!
- Release: https://github.com/dfinity/icp-cli/releases/tag/v$ARGUMENTS
- npm: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$ARGUMENTS
- Homebrew (homebrew-core PR, may take a few hours): https://github.com/Homebrew/homebrew-core/pulls?q=is%3Apr+icp-cli+$ARGUMENTS
```

If `$ARGUMENTS` is a beta release, output:
```
🚀 icp-cli v$ARGUMENTS released!
- Release: https://github.com/dfinity/icp-cli/releases/tag/v$ARGUMENTS
- npm: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$ARGUMENTS
- Homebrew: `brew install dfinity/tap/icp-cli-beta`
```
