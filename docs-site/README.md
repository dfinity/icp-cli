# ICP CLI Documentation Site

This directory contains the Starlight-based documentation website for ICP CLI.

## Overview

The documentation site is built with [Astro](https://astro.build/) and [Starlight](https://starlight.astro.build/), reading markdown files from the `../docs/` directory.

## Architecture

```
docs-site/
├── astro.config.mjs     # Starlight configuration (sidebar, theme)
├── src/
│   ├── content.config.ts  # Content loader configuration
│   ├── assets/            # Logo and static assets
│   └── styles/            # DFINITY theme CSS
├── public/              # Static files (favicon, etc.)
└── package.json         # Dependencies and scripts
```

## Key Features

### Content Loading
- Uses Astro's `glob` loader to read from `../docs/` via a temporary `.docs-temp/` directory
- Source docs remain plain Markdown (GitHub-friendly)
- Build process adds frontmatter and processes links automatically

### Build Pipeline

The build pipeline ensures source documentation remains clean while producing a polished site:

**Step 1: Prepare Docs** (`../scripts/prepare-docs.sh`)
- Copies `../docs/` to `.docs-temp/` (excluding `schemas/` directory and README files)
- Adjusts relative paths and strips `.md` extensions for Starlight's clean URLs
  - Source files use `.md` extensions (work on GitHub)
  - Build transforms to clean URLs without `.md` (work on site)
- Extracts page title from first H1 heading
- Adds YAML frontmatter with the title
- Removes the H1 heading from content (prevents duplicate titles)

**Step 2: Starlight Build**
- Reads content from `.docs-temp/` via glob loader
- Applies DFINITY theme CSS
- Generates navigation from manual sidebar configuration
- Produces static HTML in `dist/`

**Why this approach?**
- Source docs stay plain Markdown (GitHub-friendly, no framework lock-in)
- Build-time transformations keep things DRY (single source of truth)
- Cross-platform compatibility (works on macOS and Linux)

### Styling
- Custom CSS copied from `icp-js-sdk-docs` for DFINITY branding
- Files: `layers.css`, `theme.css`, `overrides.css`, `elements.css`
- Maintains consistent look with other DFINITY documentation sites

### External Links
- External links automatically open in new tabs with security attributes (`rel="noopener noreferrer"`)
- Implemented via `rehype-external-links` plugin for content links
- Custom script in `astro.config.mjs` handles social/header links

### Navigation
- Sidebar is **manually configured** in `astro.config.mjs`
- This is required because Starlight's autogenerate doesn't work with glob loaders
- When adding new docs, update the sidebar configuration

## Development

### Prerequisites
```bash
npm install
```

### Local Development
```bash
npm run dev
```
Opens the site at `http://localhost:4321`

### Build for Production
```bash
npm run build
```
Outputs to `./dist/`

### Preview Production Build
```bash
npm run preview
```

### Clean Build Artifacts
```bash
npm run clean
```
Removes `.docs-temp/`, `dist/`, and `.astro/` directories

## Scripts

- `prepare-docs` - Runs `../scripts/prepare-docs.sh` to prepare documentation files
- `dev` - Cleans artifacts, prepares docs, and starts development server
- `build` - Prepares docs and builds for production
- `preview` - Previews production build locally
- `clean` - Removes build artifacts (`.docs-temp/`, `dist/`, `.astro/`)

## Deployment

The site is automatically deployed to GitHub Pages:
- **URL**: https://dfinity.github.io/icp-cli/
- **Workflow**: `.github/workflows/docs.yml`
- **Trigger**: Push to `main` branch (docs or docs-site changes)

The workflow:
1. Installs dependencies
2. Runs `npm run build` (which runs `prepare-docs.sh`)
3. Uploads the `dist/` directory as a GitHub Pages artifact
4. Deploys to GitHub Pages

## Configuration

### Site Settings
In `astro.config.mjs`:
- `site`: Base URL for the site
- `base`: Base path (currently `/` for root domain)
- `title`, `description`: Site metadata
- `logo`: ICP logo configuration
- `favicon`: Site favicon
- `customCss`: DFINITY theme files
- `markdown.rehypePlugins`: External link handling with `rehype-external-links`

### Sidebar Configuration
Manual sidebar definition in `astro.config.mjs`:
```js
sidebar: [
  {
    label: 'Section Name',
    items: [
      { label: 'Page Title', slug: 'path/to/page' },
      // ...
    ],
  },
  // ...
]
```

The `slug` should match the file path relative to `docs/` without the `.md` extension.

## Adding New Pages

1. Create a `.md` file in `../docs/` in the appropriate directory
2. Start with a `# Heading` (used as page title)
3. Write standard Markdown content
4. Add the page to the sidebar in `astro.config.mjs`:
   ```js
   {
     label: 'Your Section',
     items: [
       { label: 'Your New Page', slug: 'section/your-new-page' },
       // ...
     ],
   }
   ```

## Troubleshooting

### Sidebar shows no pages
Check that:
- The file exists in `../docs/` with correct path
- The slug in `astro.config.mjs` matches the file path (without `.md`)
- You ran `npm run dev` to trigger the build process

### Duplicate page titles
Check that:
- The source file in `../docs/` has only one H1 heading
- The `prepare-docs.sh` script is running correctly

### Broken links
- Use relative links with `.md` extension in source docs: `[text](./file.md)`
- The build process (`prepare-docs.sh`) adjusts paths and strips `.md` extensions
- External links should use full URLs

## Notes

- The `.docs-temp/` directory is generated at build time and should not be committed
- Source documentation in `../docs/` should never have frontmatter
- The `schemas/` directory is excluded from the docs site (served via GitHub raw URLs)
