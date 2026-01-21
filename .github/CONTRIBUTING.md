# Contributing

Thank you for your interest in contributing to icp-cli for the Internet Computer.
By participating in this project, you agree to abide by our [Code of Conduct](./CODE_OF_CONDUCT.md).

As a member of the community, you are invited and encouraged to contribute by submitting issues, offering suggestions for improvements, adding review comments to existing pull requests, or creating new pull requests to fix issues.

All contributions to DFINITY documentation and the developer community are respected and appreciated.
Your participation is an important factor in the success of the Internet Computer.

## Before you contribute

Before contributing, please take a few minutes to review these contributor guidelines.
The contributor guidelines are intended to make the contribution process easy and effective for everyone involved in addressing your issue, assessing changes, and finalizing your pull requests.

Before contributing, consider the following:

- If you want to report an issue, click [issues](https://github.com/dfinity/icp-cli/issues).
- If you have more general questions related to this package and its use, post a message to the [community forum](https://forum.dfinity.org/).
- If you are reporting a bug, provide as much information about the problem as possible.
- If you want to contribute directly to this repository, typical fixes might include any of the following:
  - Fixes to resolve bugs or documentation errors
  - Code improvements
  - Feature requests
  - Note that any contribution to this repository must be submitted in the form of a **pull request**.
- If you are creating a pull request, be sure that the pull request only implements one fix or suggestion.

If you are new to working with GitHub repositories and creating pull requests, consider exploring [First Contributions](https://github.com/firstcontributions/first-contributions) or [How to Contribute to an Open Source Project on GitHub](https://egghead.io/courses/how-to-contribute-to-an-open-source-project-on-github).

# How to make a contribution

Depending on the type of contribution you want to make, you might follow different workflows.

This section describes the most common workflow scenarios:

- Reporting an issue
- Submitting a pull request

### Reporting an issue

To open a new issue:

1. Click [create a new issue](https://github.com/dfinity/icp-cli/issues/new).
2. Type a title and description, then click **Submit new issue**.
   - Be as clear and descriptive as possible.
   - For any problem, describe it in detail, including details about the crate, the version of the code you are using, the results you expected, and how the actual results differed from your expectations.

### Submitting a pull request

If you want to submit a pull request to fix an issue or add a feature, here's a summary of what you need to do:

1. Make sure you have a GitHub account, an internet connection, and access to a terminal shell or GitHub Desktop application for running commands.
2. Navigate to the [repository's homepage](https://github.com/dfinity/icp-cli) in a web browser.
3. Click **Fork** to create a copy of the repository under your GitHub account or organization name.
4. Clone the forked repository to your local machine.
5. Create a new branch for your fix by running a command similar to the following:
   ```shell
   git checkout -b my-branch-name-here
   ```
6. Open the file you want to fix in a text editor and make the appropriate changes for the issue you are trying to address.
7. Add the file contents of the changed files to the index `git` uses to manage the state of the project by running a command similar to the following:
   ```shell
   git add path-to-changed-file
   ```
8. Commit your changes to store the contents you added to the index along with a descriptive message by running a command similar to the following:
   ```shell
   cz commit
   ```
   - See [Conventional commits](https://www.conventionalcommits.org/en/v1.0.0/) for more information on the commit message formats.
9. Push the changes to the remote repository by running a command similar to the following:
   ```shell
   git push origin my-branch-name-here
   ```
10. Create a new pull request (PR) for the branch you pushed to the upstream GitHub repository.
    - The PR title should be auto-populated based on your commit message.
    - Provide a PR message that includes a short description of the changes made.
11. Wait for the pull request to be reviewed.
12. Make changes to the pull request, if requested.
13. Celebrate your success after your pull request is merged!

## Contributing to Documentation

The documentation lives in the `docs/` directory and is deployed to https://dfinity.github.io/icp-cli/.

### Documentation Structure

Documentation follows the [Diátaxis framework](https://diataxis.fr/):
- `docs/guides/` - Task-oriented how-to guides
- `docs/concepts/` - Understanding-oriented explanations
- `docs/reference/` - Information-oriented technical specifications
- `docs/migration/` - Migration guides (e.g., from dfx)

### How Documentation is Built

The documentation site uses [Astro](https://astro.build/) with [Starlight](https://starlight.astro.build/):

1. **Source files** (`docs/`) are plain Markdown without frontmatter
2. **Build script** (`scripts/prepare-docs.sh`) runs before each build and:
   - Copies docs to `.docs-temp/` directory (excluding schemas and READMEs)
   - Adjusts relative paths for Starlight's `/category/page/` URL structure
   - Keeps `.md` extensions (Starlight strips them automatically; better GitHub compatibility)
   - Extracts titles from H1 headings and adds frontmatter
3. **Starlight** reads from `.docs-temp/` and builds the site
4. **GitHub Actions** automatically deploys to GitHub Pages on push to main

This architecture keeps source docs clean and GitHub-friendly while providing a polished documentation site.

### Writing Documentation

1. **Create a markdown file** in the appropriate directory:
   ```bash
   # Example: Add a new guide
   touch docs/guides/my-new-guide.md
   ```

2. **Start with an H1 heading** (used as the page title):
   ```markdown
   # My New Guide

   Content here...
   ```

3. **Use plain Markdown** - No frontmatter needed in source files:
   - Standard GitHub-flavored Markdown
   - Relative links with `.md` extension: `[text](./other-doc.md)`
   - Code blocks with language: ` ```bash `
   - The build process handles transformations automatically

4. **Add to the sidebar** - Update `docs-site/astro.config.mjs`:
   ```js
   sidebar: [
     {
       label: 'Guides',
       items: [
         { label: 'My New Guide', slug: 'guides/my-new-guide' },
         // ...
       ],
     },
   ]
   ```

   Note: The sidebar must be manually updated because Starlight's autogenerate feature doesn't work with Astro's glob loader.

5. **Preview your changes locally**:
   ```bash
   cd docs-site
   npm install  # First time only
   npm run dev
   ```
   Opens the site at http://localhost:4321

### Generated Documentation

Some documentation is auto-generated:

- **CLI reference** (`docs/reference/cli.md`) - Run `./scripts/generate-cli-docs.sh` when commands change
- **Config schemas** (`docs/schemas/*.json`) - Run `./scripts/generate-config-schemas.sh` when manifest types change

These scripts should be run before committing changes to code that affects CLI commands or configuration types.

### Documentation Guidelines

- **Keep it simple** - Plain Markdown is easier to maintain and renders well on GitHub
- **Be concise** - Users value clear, direct explanations
- **Use examples** - Show concrete code examples rather than abstract descriptions
- **Test your examples** - Make sure code examples actually work
- **Link related docs** - Help users discover related content
- **Follow Diátaxis** - Place content in the correct category:
  - **Tutorial**: Learning-oriented, takes users by the hand
  - **Guides**: Task-oriented, shows how to solve specific problems
  - **Concepts**: Understanding-oriented, explains how things work
  - **Reference**: Information-oriented, technical descriptions

### Documentation Pull Requests

When submitting a documentation PR:
- Ensure the sidebar in `docs-site/astro.config.mjs` is updated if adding new pages
- Preview the site locally before submitting
- Check that all links work (both in GitHub and on the site)
- Follow the Diátaxis framework for placing content in the right section
- Verify your examples work by testing them
- Run `./scripts/prepare-docs.sh` locally to check for build errors

For more details on the documentation system, see:
- [docs/README.md](../docs/README.md) - Documentation writing guide
- [docs-site/README.md](../docs-site/README.md) - Technical documentation site details
