# Rollback guidance

If something fails mid-release, here's how to clean up depending on how far you got:

- **Task 1 failed (PR)**: Close the PR and delete the branch. No external state was changed.
- **Task 2 failed (tag pushed but workflows fail)**: The tag can be deleted if the Release workflow hasn't created a GitHub Release yet:
  ```bash
  git push origin --delete v$ARGUMENTS
  git tag -d v$ARGUMENTS
  ```
  If a GitHub Release was already created, delete it first via `gh release delete v$ARGUMENTS --yes`, then delete the tag.
  Also revert the `docs/vX.Y` tag that `sync-docs-tag.yml` moved automatically. If a previous patch existed (e.g. rolling back v0.2.3 means docs/v0.2 should revert to v0.2.2), force-move it back:
  ```bash
  MINOR=$(echo "$ARGUMENTS" | sed 's/\.[0-9]*$//')
  git tag -f docs/v${MINOR} v${MINOR}.$(( $(echo "$ARGUMENTS" | sed 's/.*\.//') - 1 ))
  git push origin refs/tags/docs/v${MINOR} --force
  ```
  If this was the first release of the minor (no previous patch), delete the docs tag instead:
  ```bash
  MINOR=$(echo "$ARGUMENTS" | sed 's/\.[0-9]*$//')
  git push origin --delete refs/tags/docs/v${MINOR}
  git tag -d docs/v${MINOR}
  ```
- **Task 3 failed (Release workflow)**: Investigate the failure. The tag still exists. Once fixed, you can re-run the workflow from the GitHub Actions UI. Do **not** delete and re-push the tag — that creates duplicate runs.
- **Task 4 failed (NPM publish)**: NPM publishes are not easily reversible. If the publish partially succeeded, check `npm info @icp-sdk/icp-cli versions` and coordinate with the team. The workflow can be re-triggered from the GitHub Actions UI.
- **Task 5 failed (homebrew-tap)**: If the workflow failed, it can be re-triggered. If the PR was created but has issues, close it and delete the branch `update/icp-cli-beta-$ARGUMENTS` on `dfinity/homebrew-tap` via the GitHub UI. No packages were published.
- **Task 6 failed (docs versions)**: Close the versions.json PR and delete the branch. The versioned docs at `/X.Y/` are deployed independently by the tag push and are unaffected. The `docs/vX.Y` tag was already moved by `sync-docs-tag.yml` as part of Task 2 — no additional cleanup needed for the tag unless you are also rolling back Task 2.
- **Task 7 (homebrew-core check)**: This task is read-only — no cleanup needed. If it fails, just check manually.
