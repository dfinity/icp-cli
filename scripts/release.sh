#!/bin/bash
#
# Release icp-cli.
#
# Run locally by the release driver:
#
#     ./scripts/release.sh <VERSION>          # e.g. ./scripts/release.sh 1.1.0
#
# The driver only has to approve two (sometimes three) PRs; everything else is
# driven from here. The flow:
#
#   1. Open the version-bump PR (authored by the release bot so you can approve it),
#      wait for CI + your approval, then merge it.
#   2. Tag the merge commit and push the tag (triggers the cargo-dist Release workflow).
#   3. Watch the Release workflow build artifacts and create the GitHub Release.
#   4. Publish to npm.
#   5. Publish to dfinity/homebrew-tap (approve + merge its PR).
#   6. For a new minor, update docs-site/versions.json (approve + merge its PR),
#      after the versioned docs have deployed.
#
# There is no automatic rollback. If a step fails the script stops and tells you
# exactly what failed and where to look so you can act manually. Re-running with
# the same version reuses any PR/tag that already exists.

set -uo pipefail

REPO="dfinity/icp-cli"
TAP_REPO="dfinity/homebrew-tap"
TAP_WORKFLOW="update-icp-cli-beta.yml"
POLL_SECONDS=15

# --- output helpers ---------------------------------------------------------
info()  { printf '   %s\n' "$*"; }
step()  { printf '\n==> %s\n' "$*"; }
warn()  { printf 'WARN: %s\n' "$*" >&2; }
fail()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

confirm() {
  local reply
  read -r -p "$* [y/N] " reply
  [[ "$reply" =~ ^[Yy]$ ]] || fail "Aborted by user."
}

# --- workflow / run helpers -------------------------------------------------

# Watch the most recent run of a workflow that was triggered by a tag/branch.
watch_run_by_branch() {  # <workflow-file> <branch> [repo]
  local wf="$1" branch="$2" repo="${3:-$REPO}" id=""
  for _ in $(seq 1 12); do
    id=$(gh run list --repo "$repo" --workflow "$wf" --branch "$branch" --limit 1 \
      --json databaseId --jq '.[0].databaseId // ""')
    [[ -n "$id" ]] && break
    sleep 5
  done
  [[ -n "$id" ]] || fail "Could not find a '$wf' run for '$branch' in $repo. Check https://github.com/$repo/actions"
  info "Watching: https://github.com/$repo/actions/runs/$id"
  gh run watch --repo "$repo" "$id" --exit-status \
    || fail "'$wf' failed for $branch: https://github.com/$repo/actions/runs/$id"
}

# Dispatch a workflow_dispatch workflow and watch the run it creates.
dispatch_and_watch() {  # <workflow-file> <repo> <field=value>...
  local wf="$1" repo="$2"; shift 2
  local fields=() f prev id
  for f in "$@"; do fields+=(--field "$f"); done
  prev=$(gh run list --repo "$repo" --workflow "$wf" --limit 1 --json databaseId --jq '.[0].databaseId // ""')
  gh workflow run "$wf" --repo "$repo" "${fields[@]}" \
    || fail "Failed to dispatch '$wf' in $repo."
  for _ in $(seq 1 12); do
    id=$(gh run list --repo "$repo" --workflow "$wf" --limit 1 --json databaseId --jq '.[0].databaseId // ""')
    [[ -n "$id" && "$id" != "$prev" ]] && break
    id=""; sleep 5
  done
  [[ -n "$id" ]] || fail "Dispatched '$wf' in $repo but no new run appeared. Check https://github.com/$repo/actions"
  info "Watching: https://github.com/$repo/actions/runs/$id"
  gh run watch --repo "$repo" "$id" --exit-status \
    || fail "'$wf' failed in $repo: https://github.com/$repo/actions/runs/$id"
}

# --- PR helpers -------------------------------------------------------------

pr_url() {     # <branch> [repo]
  gh pr view "$1" --repo "${2:-$REPO}" --json url --jq '.url' 2>/dev/null
}

wait_for_checks() {  # <branch> [repo]
  local branch="$1" repo="${2:-$REPO}"
  info "Waiting for CI checks..."
  gh pr checks "$branch" --repo "$repo" --watch \
    || fail "CI is failing on $(pr_url "$branch" "$repo"). Fix it (or re-run flaky checks), then re-run this script."
}

wait_for_approval_and_merge() {  # <branch> [repo]
  local branch="$1" repo="${2:-$REPO}" url decision state
  url=$(pr_url "$branch" "$repo")
  printf '\n   >>> Please review and APPROVE the PR: %s\n' "$url"
  while true; do
    decision=$(gh pr view "$branch" --repo "$repo" --json reviewDecision --jq '.reviewDecision // ""')
    state=$(gh pr view "$branch" --repo "$repo" --json state --jq '.state')
    [[ "$state" == "MERGED" ]] && { info "Already merged."; return 0; }
    [[ "$state" != "OPEN" ]] && fail "PR is $state (expected OPEN): $url"
    [[ "$decision" == "APPROVED" ]] && break
    [[ "$decision" == "CHANGES_REQUESTED" ]] && fail "Changes were requested on $url. Resolve them, then re-run this script."
    info "Not approved yet (review: ${decision:-none}). Checking again in ${POLL_SECONDS}s..."
    sleep "$POLL_SECONDS"
  done
  info "Approved — merging."
  gh pr merge "$branch" --repo "$repo" --squash --delete-branch \
    || fail "Merge failed for $url. Check its status and merge manually if needed."
}

# Open a release PR via the release-pr.yml workflow unless one already exists.
ensure_release_pr() {  # <kind> <branch>
  local kind="$1" branch="$2" existing
  existing=$(gh pr list --repo "$REPO" --head "$branch" --state open --json url --jq '.[0].url // ""')
  if [[ -n "$existing" ]]; then
    info "Reusing existing PR: $existing"
    return 0
  fi
  dispatch_and_watch release-pr.yml "$REPO" "kind=$kind" "version=$VERSION"
  for _ in $(seq 1 12); do
    existing=$(gh pr list --repo "$REPO" --head "$branch" --state open --json url --jq '.[0].url // ""')
    [[ -n "$existing" ]] && break
    sleep 5
  done
  [[ -n "$existing" ]] || fail "release-pr.yml ran but no PR appeared on '$branch'. Check https://github.com/$REPO/actions"
  info "Opened PR: $existing"
}

# ============================================================================
# Preflight
# ============================================================================
VERSION="${1:-}"
VERSION="${VERSION#v}"
[[ -n "$VERSION" ]] || fail "Usage: $0 <VERSION>   (e.g. $0 1.1.0)"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "'$VERSION' is not a valid stable version (expected X.Y.Z)."
MINOR="${VERSION%.*}"
TAG="v$VERSION"

for tool in git gh jq; do
  command -v "$tool" >/dev/null 2>&1 || fail "'$tool' is required but not installed."
done
gh auth status >/dev/null 2>&1 || fail "Not logged in to GitHub. Run 'gh auth login' first."

step "Releasing icp-cli $TAG"
info "Repo:           $REPO"
info "Homebrew tap:   $TAP_REPO"
info "Docs minor:     $MINOR"
confirm "Proceed?"

# ============================================================================
# 1. Version-bump PR
# ============================================================================
step "Step 1/6 — Version-bump PR"
git fetch -q origin main || fail "Could not fetch origin/main."
main_version=$(git show origin/main:Cargo.toml \
  | awk -F'"' '/^\[/{s=$0} s=="[workspace.package]"&&/^version[[:space:]]*=/{print $2; exit}')
if [[ "$main_version" == "$VERSION" ]]; then
  info "origin/main is already at $VERSION — bump already merged, skipping step 1."
else
  ensure_release_pr version-bump "release/$TAG"
  wait_for_checks "release/$TAG"
  wait_for_approval_and_merge "release/$TAG"
fi

# ============================================================================
# 2. Tag and push
# ============================================================================
step "Step 2/6 — Tag and push"
git checkout main >/dev/null 2>&1 || fail "Could not switch to main."
git pull --ff-only origin main || fail "Could not fast-forward main. Resolve manually and re-run."

actual=$(awk -F'"' '/^\[/{s=$0} s=="[workspace.package]"&&/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)
[[ "$actual" == "$VERSION" ]] || fail "main's Cargo.toml version is '$actual', expected '$VERSION'. Did the bump PR merge?"

if git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1; then
  warn "Tag $TAG already exists on origin — skipping tag push."
else
  confirm "Push tag $TAG? This triggers the Release workflow and is hard to undo."
  git tag "$TAG" || fail "Could not create tag $TAG."
  git push origin "$TAG" || fail "Could not push tag $TAG."
  info "Pushed $TAG."
fi

# ============================================================================
# 3. Release workflow (cargo-dist)
# ============================================================================
step "Step 3/6 — Release workflow (builds artifacts + GitHub Release)"
watch_run_by_branch release.yml "$TAG"
info "GitHub Release: https://github.com/$REPO/releases/tag/$TAG"

# ============================================================================
# 4. Publish to npm
# ============================================================================
step "Step 4/6 — Publish to npm"
dispatch_and_watch release-npm.yml "$REPO" "version=$TAG" "npm_package_version=$VERSION"
info "NPM: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$VERSION"

# ============================================================================
# 5. Publish to homebrew-tap
# ============================================================================
step "Step 5/6 — Publish to dfinity/homebrew-tap"
dispatch_and_watch "$TAP_WORKFLOW" "$TAP_REPO" "version=$VERSION"
wait_for_approval_and_merge "update/icp-cli-beta-$VERSION" "$TAP_REPO"

# ============================================================================
# 6. Docs versions.json (new minor only)
# ============================================================================
step "Step 6/6 — Docs site versions"
current_latest=$(jq -r '[.versions[] | select(.latest==true) | .version]
  | if length==1 then .[0] elif length==0 then "" else "MULTIPLE" end' docs-site/versions.json) \
  || fail "Could not parse docs-site/versions.json."
[[ "$current_latest" == "MULTIPLE" ]] && fail "docs-site/versions.json has multiple 'latest:true' entries — fix it before releasing."
if [[ "$current_latest" == "$MINOR" ]]; then
  info "docs-site/versions.json already lists v$MINOR as latest — nothing to do."
else
  info "Waiting for the versioned docs (/$MINOR/) to deploy before updating the redirect..."
  watch_run_by_branch docs.yml "$TAG"
  ensure_release_pr docs-versions "release/docs-$TAG"
  wait_for_checks "release/docs-$TAG"
  wait_for_approval_and_merge "release/docs-$TAG"
fi

# ============================================================================
# Done — announcement
# ============================================================================
step "Release complete 🎉  — announcement for the team channel:"
cat <<EOF

🚀 icp-cli $TAG released!
- Release: https://github.com/$REPO/releases/tag/$TAG
- NPM: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$VERSION
- Homebrew (tap): \`brew install dfinity/tap/icp-cli-beta\`
- Homebrew (core): BrewTestBot picks up the new release automatically within a few hours.
EOF
