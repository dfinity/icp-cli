# Task 1: Bump the version and open a release PR

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
