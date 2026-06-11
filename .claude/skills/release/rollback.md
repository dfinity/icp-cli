# Rollback guidance

If something fails mid-release, here's how to clean up depending on how far you got:

- **Task 1 failed (PR)**: Close the PR and delete the branch. No external state was changed.
- **Task 2 failed (tag pushed but workflows fail)**: The tag can be deleted if the Release workflow hasn't created a GitHub Release yet:
  ```bash
  git push origin --delete v$ARGUMENTS
  git tag -d v$ARGUMENTS
  ```
  If a GitHub Release was already created, delete it first via `gh release delete v$ARGUMENTS --yes`, then delete the tag.
  Note: `delete-docs-branch.yml` may have already deleted the `docs/vX.Y` branch as part of this release. The live docs at `/X.Y/` are deployed independently and are unaffected. If a docs-only fix branch needs to be restored, create a fresh one from the previous release tag.
- **Task 3 failed (Release workflow)**: Investigate the failure. The tag still exists. Once fixed, you can re-run the workflow from the GitHub Actions UI. Do **not** delete and re-push the tag — that creates duplicate runs.
- **Task 4 failed (NPM publish)**: NPM publishes are not easily reversible. If the publish partially succeeded, check `npm info @icp-sdk/icp-cli versions` and coordinate with the team. The workflow can be re-triggered from the GitHub Actions UI.
- **Task 5 failed (homebrew-tap)**: If the workflow failed, it can be re-triggered. If the PR was created but has issues, close it and delete the branch `update/icp-cli-beta-$ARGUMENTS` on `dfinity/homebrew-tap` via the GitHub UI. No packages were published.
- **Task 6 failed (docs versions)**: Close the PR and delete the branch. The versioned docs are deployed independently and are unaffected.
- **Task 7 (homebrew-core check)**: This task is read-only — no cleanup needed. If it fails, just check manually.
