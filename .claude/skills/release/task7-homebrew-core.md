# Task 7: Check homebrew-core status (stable releases only)

*Skip if `$ARGUMENTS` is a beta release. Runs after all other tasks are complete, just before the release announcement.*

This task checks the status of the BrewTestBot's automatic PR to homebrew-core. This is managed externally by Homebrew — we just check its current state to include in the announcement.

Check the homebrew-core PR and extract its URL and state:
```bash
HBC_PR=$(gh pr list --repo Homebrew/homebrew-core \
  --search "icp-cli $ARGUMENTS" \
  --json number,state,url,mergedAt \
  --state all)
HBC_PR_URL=$(echo "$HBC_PR" | jq -r '.[0].url // ""')
HBC_PR_STATE=$(echo "$HBC_PR" | jq -r '.[0].state // ""')
```

Determine the **homebrew-core status line** to use in the release announcement:

- If `$HBC_PR_URL` is empty (no PR found):
  `- Homebrew (core): BrewTestBot hasn't created the PR yet, check https://github.com/Homebrew/homebrew-core/pulls?q=is%3Apr+icp-cli+$ARGUMENTS later`
- If `$HBC_PR_STATE` is `OPEN`:
  `- Homebrew (core): formula PR is in review: $HBC_PR_URL`
- If `$HBC_PR_STATE` is `MERGED`: check whether the new version is live:
  ```bash
  curl -sf https://formulae.brew.sh/api/formula/icp-cli.json | jq -r '.versions.stable'
  ```
  - If the returned version equals `$ARGUMENTS`:
    `- Homebrew (core): published. \`brew install icp-cli\` (or \`brew upgrade icp-cli\`)`
  - If the returned version does not equal `$ARGUMENTS`:
    `- Homebrew (core): formula PR merged but not yet propagated: $HBC_PR_URL`

Proceed to the release announcement with the homebrew-core status line determined above.
