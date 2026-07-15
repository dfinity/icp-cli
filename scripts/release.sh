#!/bin/bash
#
# Release icp-cli.
#
# Run locally by the release driver:
#
#     ./scripts/release.sh <VERSION>          # e.g. ./scripts/release.sh 1.1.0
#
# The only thing the driver does by hand is APPROVE two (sometimes three) PRs;
# everything else is driven from here. The flow:
#
#   1. Open the version-bump PR (authored by the release bot so you can approve it);
#      approve it and auto-merge lands it once CI is green.
#   2. Tag the merge commit and push the tag (triggers the cargo-dist Release workflow).
#   3. Watch the Release workflow build artifacts and create the GitHub Release.
#   4. Publish to npm.
#   5. Publish to dfinity/homebrew-tap (approve + merge its PR).
#   6. For a new minor, update docs-site/versions.json (approve + merge its PR),
#      after the versioned docs have deployed.
#
# Every step is idempotent: it first checks whether the work is already done
# (version on npm, formula bumped, docs entry present, tag pushed, PR merged) and
# skips it if so; if a workflow run or PR is already in flight it watches that one
# instead of starting a new one. So re-running with the same version after a
# failure picks up exactly where it left off.
#
# PR merges use auto-merge: once you approve, the merge happens the moment the
# required checks go green — you don't have to wait for CI before approving, and
# approving early (or enabling auto-merge yourself) won't trip up the script.
#
# There is no automatic rollback. If a step fails the script stops and tells you
# exactly what failed and where to look so you can act manually.

set -uo pipefail

REPO="dfinity/icp-cli"
TAP_REPO="dfinity/homebrew-tap"
TAP_WORKFLOW="update-icp-cli-beta.yml"
NPM_PKG="@icp-sdk/icp-cli"
POLL_SECONDS=15
# After you approve, the script turns on auto-merge and waits for it to land,
# sitting through CI. If the checks finish but the PR still hasn't merged within
# MERGE_STALL_SECONDS, a required check probably failed — bail so a human can look.
# MERGE_TIMEOUT_SECONDS is an absolute backstop against a check stuck 'pending'.
MERGE_STALL_SECONDS=300
MERGE_TIMEOUT_SECONDS=7200

# --- output helpers ---------------------------------------------------------
info()  { printf '   %s\n' "$*"; }
step()  { printf '\n==> %s\n' "$*"; }
warn()  { printf 'WARN: %s\n' "$*" >&2; }
fail()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

# --- state probes -----------------------------------------------------------

# Is <version> already published on the npm registry?
npm_published() {  # <version>
  local code
  code=$(curl -sS -o /dev/null -w '%{http_code}' "https://registry.npmjs.org/$NPM_PKG/$1" 2>/dev/null)
  [[ "$code" == "200" ]]
}

# Current version pinned in the homebrew formula (empty if it can't be read).
tap_formula_version() {
  gh api "repos/$TAP_REPO/contents/Formula/icp-cli-beta.rb" --jq '.content' 2>/dev/null \
    | base64 -d 2>/dev/null \
    | awk -F'"' '/^[[:space:]]*ver[[:space:]]*=/{print $2; exit}'
}

# databaseId of an in-flight (queued/running) run of <workflow>, or "" if none.
ongoing_run_id() {  # <workflow-file> [repo]
  gh run list --repo "${2:-$REPO}" --workflow "$1" --limit 20 --json databaseId,status \
    --jq 'map(select(.status=="queued" or .status=="in_progress" or .status=="requested" or .status=="waiting" or .status=="pending")) | .[0].databaseId // ""'
}

# URL of the open PR with the given head branch, or "" if none.
open_pr_url() {  # <branch> [repo]
  gh pr list --repo "${2:-$REPO}" --head "$1" --state open --json url --jq '.[0].url // ""'
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

# If a run of <wf> is already in flight, watch that one; otherwise dispatch a
# fresh one and watch it. Avoids kicking off a duplicate run on a re-run.
ensure_workflow_run() {  # <workflow-file> <repo> <field=value>...
  local wf="$1" repo="$2"; shift 2
  local id
  id=$(ongoing_run_id "$wf" "$repo")
  if [[ -n "$id" ]]; then
    info "A '$wf' run is already in flight — watching it instead of dispatching a new one."
    info "Watching: https://github.com/$repo/actions/runs/$id"
    gh run watch --repo "$repo" "$id" --exit-status \
      || fail "'$wf' failed in $repo: https://github.com/$repo/actions/runs/$id"
    return 0
  fi
  dispatch_and_watch "$wf" "$repo" "$@"
}

# --- PR helpers -------------------------------------------------------------

pr_url() {     # <branch> [repo]
  gh pr view "$1" --repo "${2:-$REPO}" --json url --jq '.url' 2>/dev/null
}

# Are any of the PR's checks still running/queued? False if it has no checks yet
# or the checks have all concluded (pass/fail/skip/cancel).
checks_pending() {  # <branch> [repo]
  local n
  n=$(gh pr checks "$1" --repo "${2:-$REPO}" --json bucket --jq '[.[] | select(.bucket=="pending")] | length' 2>/dev/null)
  [[ "${n:-0}" -gt 0 ]]
}

# Wait for a PR to be approved, then merge it (via auto-merge so CI need not be
# green yet), then wait for the merge to actually land. Idempotent: returns
# happily if the PR is already merged or the reviewer already enabled auto-merge.
wait_for_approval_and_merge() {  # <branch> [repo]
  local branch="$1" repo="${2:-$REPO}" url decision state auto waited=0 stalled=0

  # The PR may have just been opened by a workflow — give it a moment to appear.
  for _ in $(seq 1 12); do
    url=$(pr_url "$branch" "$repo")
    [[ -n "$url" ]] && break
    sleep 5
  done
  [[ -n "$url" ]] || fail "Expected a PR on '$branch' in $repo but found none. Check https://github.com/$repo/pulls"

  # 1. Wait until the PR is approved (or already merged).
  printf '\n   >>> Please review and APPROVE the PR: %s\n' "$url"
  while true; do
    state=$(gh pr view "$branch" --repo "$repo" --json state --jq '.state' 2>/dev/null)
    [[ "$state" == "MERGED" ]] && { info "Already merged."; return 0; }
    [[ "$state" == "CLOSED" ]] && fail "PR is CLOSED (expected OPEN): $url"
    decision=$(gh pr view "$branch" --repo "$repo" --json reviewDecision --jq '.reviewDecision // ""' 2>/dev/null)
    [[ "$decision" == "APPROVED" ]] && break
    [[ "$decision" == "CHANGES_REQUESTED" ]] && fail "Changes were requested on $url. Resolve them, then re-run this script."
    info "Not approved yet (review: ${decision:-none}). Checking again in ${POLL_SECONDS}s..."
    sleep "$POLL_SECONDS"
  done

  # 2. Enable auto-merge (squash). Merges immediately if the required checks are
  #    already green, otherwise queues the merge for when they pass. This is why
  #    approving while CI is still running is fine. If the reviewer already turned
  #    on auto-merge by hand, there's nothing to do.
  auto=$(gh pr view "$branch" --repo "$repo" --json autoMergeRequest --jq '.autoMergeRequest != null' 2>/dev/null)
  if [[ "$auto" == "true" ]]; then
    info "Approved — auto-merge already enabled; waiting for it to complete."
  else
    info "Approved — enabling auto-merge (squash)."
    gh pr merge "$branch" --repo "$repo" --squash --auto --delete-branch || {
      # Tolerate the race where it merged in the gap after we saw APPROVED.
      state=$(gh pr view "$branch" --repo "$repo" --json state --jq '.state' 2>/dev/null)
      [[ "$state" == "MERGED" ]] || fail "Could not enable auto-merge for $url. Check its status and merge it manually if needed, then re-run this script."
    }
  fi

  # 3. Wait for the merge to actually land — later steps (e.g. tagging main)
  #    depend on it. Auto-merge fires once the required checks pass, so we sit
  #    through CI here (however long it takes). We give up only if the checks
  #    have concluded and the PR still hasn't merged for MERGE_STALL_SECONDS
  #    (a required check likely failed / the merge is blocked), or after the
  #    absolute MERGE_TIMEOUT_SECONDS backstop.
  while true; do
    state=$(gh pr view "$branch" --repo "$repo" --json state --jq '.state' 2>/dev/null)
    [[ "$state" == "MERGED" ]] && { info "Merged."; return 0; }
    [[ "$state" == "CLOSED" ]] && fail "PR was closed before it merged: $url"
    if checks_pending "$branch" "$repo"; then
      stalled=0
      info "CI running — auto-merge will land it once the required checks pass (${POLL_SECONDS}s)..."
    else
      stalled=$((stalled + POLL_SECONDS))
      (( stalled >= MERGE_STALL_SECONDS )) && fail "Checks finished but $url didn't auto-merge within $((MERGE_STALL_SECONDS / 60))m.
   A required check probably failed, or the merge is blocked. Check the PR, get
   it merged, then re-run this script."
      info "Checks finished; waiting for auto-merge to land (${POLL_SECONDS}s)..."
    fi
    (( waited >= MERGE_TIMEOUT_SECONDS )) && fail "PR still not merged after $((MERGE_TIMEOUT_SECONDS / 60))m: $url
   Check the PR and merge it manually if needed, then re-run this script."
    sleep "$POLL_SECONDS"
    waited=$((waited + POLL_SECONDS))
  done
}

# Open a release PR via the release-pr.yml workflow unless one already exists.
ensure_release_pr() {  # <kind> <branch>
  local kind="$1" branch="$2" existing
  existing=$(open_pr_url "$branch")
  if [[ -n "$existing" ]]; then
    info "Reusing existing PR: $existing"
    return 0
  fi
  dispatch_and_watch release-pr.yml "$REPO" "kind=$kind" "version=$VERSION"
  for _ in $(seq 1 12); do
    existing=$(open_pr_url "$branch")
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

for tool in git gh jq curl; do
  command -v "$tool" >/dev/null 2>&1 || fail "'$tool' is required but not installed."
done
gh auth status >/dev/null 2>&1 || fail "Not logged in to GitHub. Run 'gh auth login' first."

step "Releasing icp-cli $TAG"
info "Repo:           $REPO"
info "Homebrew tap:   $TAP_REPO"
info "Docs minor:     $MINOR"

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
if npm_published "$VERSION"; then
  info "$NPM_PKG@$VERSION is already on npm — skipping publish."
else
  ensure_workflow_run release-npm.yml "$REPO" "version=$TAG" "npm_package_version=$VERSION"
fi
info "NPM: https://www.npmjs.com/package/$NPM_PKG/v/$VERSION"

# ============================================================================
# 5. Publish to homebrew-tap
# ============================================================================
step "Step 5/6 — Publish to dfinity/homebrew-tap"
tap_branch="update/icp-cli-beta-$VERSION"
if [[ "$(tap_formula_version)" == "$VERSION" ]]; then
  info "homebrew formula icp-cli-beta is already at $VERSION — skipping."
elif [[ -n "$(open_pr_url "$tap_branch" "$TAP_REPO")" ]]; then
  info "Formula update PR is already open — waiting for it to merge."
  wait_for_approval_and_merge "$tap_branch" "$TAP_REPO"
else
  ensure_workflow_run "$TAP_WORKFLOW" "$TAP_REPO" "version=$VERSION"
  wait_for_approval_and_merge "$tap_branch" "$TAP_REPO"
fi

# ============================================================================
# 6. Docs versions.json (new minor only)
# ============================================================================
step "Step 6/6 — Docs site versions"
current_latest=$(jq -r '[.versions[] | select(.latest==true) | .version]
  | if length==1 then .[0] elif length==0 then "" else "MULTIPLE" end' docs-site/versions.json) \
  || fail "Could not parse docs-site/versions.json."
[[ "$current_latest" == "MULTIPLE" ]] && fail "docs-site/versions.json has multiple 'latest:true' entries — fix it before releasing."
docs_branch="release/docs-$TAG"
if [[ "$current_latest" == "$MINOR" ]]; then
  info "docs-site/versions.json already lists v$MINOR as latest — nothing to do."
elif [[ -n "$(open_pr_url "$docs_branch")" ]]; then
  info "Docs-versions PR is already open — waiting for it to merge."
  wait_for_approval_and_merge "$docs_branch"
else
  info "Waiting for the versioned docs (/$MINOR/) to deploy before updating the redirect..."
  watch_run_by_branch docs.yml "$TAG"
  ensure_release_pr docs-versions "$docs_branch"
  wait_for_approval_and_merge "$docs_branch"
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
