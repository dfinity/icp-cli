# ICP CLI Documentation Site

This directory contains the Starlight-based documentation website for ICP CLI.

## Overview

The documentation site is built with [Astro](https://astro.build/) and [Starlight](https://starlight.astro.build/), reading markdown files directly from the `../docs/` directory.

## Architecture

```
docs-site/
├── astro.config.mjs       # Starlight configuration (sidebar, theme)
├── plugins/
│   └── rehype-rewrite-links.mjs  # Rewrites .md links for Starlight's clean URLs
├── src/
│   ├── content.config.ts  # Content loader configuration
│   ├── components/        # Custom Starlight component overrides
│   ├── assets/            # Logo and static assets
│   └── styles/            # DFINITY theme CSS
├── public/                # Static files (favicon, etc.)
└── package.json           # Dependencies and scripts
```

## Key Features

### Content Loading
- Uses Astro's `glob` loader to read directly from `../docs/` (excluding `schemas/` and README files)
- Source docs use minimal YAML frontmatter (`title` + `description`)
- A rehype plugin rewrites `.md` links at build time for Starlight's clean URLs

### Build Pipeline

1. **Starlight** reads content directly from `../docs/` via the glob content loader
2. **Rehype plugin** (`plugins/rehype-rewrite-links.mjs`) strips `.md` extensions from relative links and adjusts paths for Astro's directory-based output
3. **DFINITY theme CSS** is applied for consistent branding
4. **Static HTML** is produced in `dist/`

Source docs use `.md` extensions in links (GitHub-friendly), and the rehype plugin transforms them to clean URLs at build time.

### Styling
- Custom CSS for DFINITY branding
- Files: `layers.css`, `theme.css`, `overrides.css`, `elements.css`
- Maintains consistent look with other DFINITY documentation sites

### External Links
- External links automatically open in new tabs with security attributes (`rel="noopener noreferrer"`)
- Implemented via `rehype-external-links` plugin for content links
- Custom script in `astro.config.mjs` handles social/header links

### Global Banner
- A feedback banner is shown on every page via a custom `Banner` component override (`src/components/Banner.astro`)
- No per-page frontmatter needed — the component renders the same banner globally

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
Removes `dist/` and `.astro/` directories

## Scripts

- `dev` - Cleans artifacts and starts development server
- `build` - Builds for production
- `preview` - Previews production build locally
- `clean` - Removes build artifacts (`dist/`, `.astro/`)

## Deployment

The site is hosted on an IC asset canister and served at `https://cli.icp.build`.

**Canister ID**: `ak73b-maaaa-aaaad-qlbgq-cai`

### How it works

1. **`.github/workflows/docs.yml`** builds documentation and pushes built files to the `docs-deployment` branch (one directory per version: `0.1/`, `0.2/`, `main/`, etc.)
2. **`.github/workflows/docs-deploy.yml`** triggers on pushes to `docs-deployment` and deploys the entire branch to the IC asset canister

### Triggers

- **Push to `main`**: Rebuilds `/main/` docs and root files (`index.html`, `versions.json`, IC config)
- **Tags (`v*`)**: Builds versioned docs (e.g., `v0.2.0` → `/0.2/`)
- **Branches (`docs/v*`)**: Updates versioned docs (e.g., `docs/v0.1` → `/0.1/`)

### Legacy redirect

The old GitHub Pages site at `https://dfinity.github.io/icp-cli/` redirects all paths to `https://cli.icp.build/`.

## Configuration

### Site Settings
In `astro.config.mjs`:
- `site`: Base URL (`https://cli.icp.build` in production)
- `base`: Version path (set via `PUBLIC_BASE_PATH`, e.g., `/0.2/`, `/main/`)
- `title`, `description`: Site metadata
- `logo`: ICP logo configuration
- `favicon`: Site favicon
- `customCss`: DFINITY theme files
- `markdown.rehypePlugins`: Link rewriting and external link handling

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
2. Add YAML frontmatter with `title` and `description`
3. Write standard Markdown content (no H1 heading — Starlight renders the title)
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
- The file has YAML frontmatter with at least a `title` field
- The slug in `astro.config.mjs` matches the file path (without `.md`)
- You ran `npm run dev` to trigger the build process

### Duplicate page titles
Check that:
- The source file in `../docs/` does not have an H1 heading (the `title` frontmatter is rendered as H1 by Starlight)

### Broken links
- Use relative links with `.md` extension in source docs: `[text](./file.md)`
- The rehype plugin (`plugins/rehype-rewrite-links.mjs`) strips `.md` extensions and adjusts paths at build time
- External links should use full URLs

## Notes

- Source documentation in `../docs/` uses minimal YAML frontmatter (`title` + `description`)
- The `schemas/` directory is excluded from the docs site (served via GitHub raw URLs)
