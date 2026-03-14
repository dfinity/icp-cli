# Task 2: Tag

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

**After the tag is pushed, start Task 3 in background. If `$ARGUMENTS` is a stable release, also start Task 6 concurrently in background.**
