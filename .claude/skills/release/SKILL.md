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
    +-------------------+
    |                   |
Task 3              Task 6
(Release workflow)  (docs site versions)
    |               [stable only]
    +-------+
    |       |
Task 4  Task 5
(NPM)   (tap)
    |
Task 7 (homebrew-core check) [stable only]
```

- **Tasks 4 & 5** require Task 3 (need GitHub release artifacts) and can start concurrently once it completes.
- **Task 6** starts immediately after Task 2 (the tag push), running concurrently with Task 3. Stable-only; must wait for the docs deployment triggered by the tag before its PR can be merged.
- **Task 7** runs last, after all other tasks complete. Stable-only — checks the Homebrew bot's homebrew-core PR status before the final announcement.

## Tasks

Follow each task's instructions in order, respecting the dependency graph above:

- **Task 1** — Bump version and open release PR: [task1-bump-pr.md](task1-bump-pr.md)
- **Task 2** — Tag after PR merge: [task2-tag.md](task2-tag.md)
- **Task 3** — Monitor Release workflow: [task3-release-wf.md](task3-release-wf.md)
- **Task 4** — Publish to NPM: [task4-npm.md](task4-npm.md)
- **Task 5** — Publish to dfinity/homebrew-tap: [task5-tap.md](task5-tap.md)
- **Task 6** — Update docs site versions (stable only): [task6-docs.md](task6-docs.md)
- **Task 7** — Check homebrew-core status (stable only): [task7-homebrew-core.md](task7-homebrew-core.md)

## Release announcement

When all tasks are complete, output a message ready to copy to the team channel.

If `$ARGUMENTS` is a stable release, output (using the homebrew-core status line from Task 7):
```
🚀 icp-cli v$ARGUMENTS released!
- Release: https://github.com/dfinity/icp-cli/releases/tag/v$ARGUMENTS
- NPM: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$ARGUMENTS
- Homebrew (tap): published to dfinity/homebrew-tap. `brew install dfinity/tap/icp-cli-beta`
- <homebrew-core status line from Task 7>
```

If `$ARGUMENTS` is a beta release, output:
```
🚀 icp-cli v$ARGUMENTS released!
- Release: https://github.com/dfinity/icp-cli/releases/tag/v$ARGUMENTS
- NPM: https://www.npmjs.com/package/@icp-sdk/icp-cli/v/$ARGUMENTS
- Homebrew (tap): published to dfinity/homebrew-tap. `brew install dfinity/tap/icp-cli-beta`
```

## Rollback guidance

If something fails mid-release, see [rollback.md](rollback.md) for cleanup instructions.
