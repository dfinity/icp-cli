# Creating Project Templates

Project templates let users scaffold new ICP projects with `icp new`. This guide covers creating custom templates for your team or the community.

## Overview

icp-cli uses [cargo-generate](https://cargo-generate.github.io/cargo-generate/) for project templating. Templates are folders or git repositories repositories containing:

- Project files with placeholder variables
- A `cargo-generate.toml` configuration file

## Quick Start

### Minimal Template

Create a basic template:

```
my-template/
├── cargo-generate.toml
├── icp.yaml
├── {{project-name}}.did
└── src/
    └── main.mo
```

**cargo-generate.toml:**

```toml
[template]
name = "My ICP Template"
description = "A simple ICP project template"
```

**icp.yaml:**

```yaml
canisters:
  - name: {{project-name}}
    recipe:
      type: "@dfinity/motoko"
      configuration:
        entry: src/main.mo
```

Filenames with handlebar placeholders like `{{project-name}}.did` will be renamed with value.

### Using Your Template

```bash
# From local directory
icp new my-project --path /path/to/my-template

# From Git repository
icp new my-project --git https://github.com/user/my-template
```

## Template Variables

### Built-in Variables

cargo-generate provides these variables automatically:

| Variable | Description |
|----------|-------------|
| `{{project-name}}` | Project name (kebab-case) |
| `{{crate_name}}` | Project name (snake_case) |
| `{{authors}}` | Git user name |

### Custom Variables

Define custom variables in `cargo-generate.toml`:

```toml
[template]
name = "My Template"

[placeholders]
include_frontend = { type = "bool", prompt = "Include frontend?", default = true }
```

Use them in templates:

```yaml
# icp.yaml
canisters:

  # ... snip snip for brevity ...

  {{#if include_frontend}}
  - name: {{project-name}}-frontend
    recipe:
      type: "@dfinity/assets"
      configuration:
        source: dist
  {{/if}}

```

## Template Structure

### Recommended Layout

```
my-template/
├── cargo-generate.toml      # Template configuration
├── icp.yaml                  # Project manifest
├── README.md                 # Project readme (templated)
├── src/
│   ├── backend/
│   │   └── main.mo          # Backend source
│   └── frontend/            # Frontend (if applicable)
│       └── index.html
└── .gitignore
```

### Configuration File

A complete `cargo-generate.toml`:

```toml
[template]
name = "Full Stack ICP App"
description = "A complete ICP application with backend and frontend"
# Exclude files from the generated project
exclude = [
    ".git",
    "target",
    ".icp"
]

[placeholders]
backend_language = { type = "string", prompt = "Backend language?", choices = ["motoko", "rust"], default = "motoko" }
include_frontend = { type = "bool", prompt = "Include frontend?", default = true }
frontend_framework = { type = "string", prompt = "Frontend framework?", choices = ["vanilla", "react", "svelte"], default = "vanilla" }

# Conditional files based on selections
[conditional]
# Include Cargo.toml only for Rust projects
"Cargo.toml" = { condition = "backend_language == 'rust'" }
"src/backend/lib.rs" = { condition = "backend_language == 'rust'" }
"src/backend/main.mo" = { condition = "backend_language == 'motoko'" }
```

## Advanced Features

### Conditional Content

Use Handlebars conditionals in any file:

```yaml
# icp.yaml
canisters:
  - name: {{project-name}}
    {{#if (eq backend_language "rust")}}
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: {{crate_name}}
    {{else}}
    recipe:
      type: "@dfinity/motoko"
      configuration:
        entry: src/backend/main.mo
    {{/if}}
```

### Conditional Files

Include files based on user choices:

```toml
# cargo-generate.toml
[conditional]
"src/frontend/" = { condition = "include_frontend" }
"package.json" = { condition = "include_frontend" }
```

### Post-Generation Hooks

Run commands after generation:

```toml
[hooks]
post = ["npm install"]
```

Note: Hooks require the user to have the necessary tools installed.

### Subfolders for Multiple Templates

Organize multiple templates in one repository:

```
icp-templates/
├── motoko-basic/
│   └── cargo-generate.toml
├── rust-basic/
│   └── cargo-generate.toml
└── full-stack/
    └── cargo-generate.toml
```

Use with `--subfolder`:

```bash
icp new my-project --git https://github.com/org/icp-templates --subfolder motoko-basic
```

## Example Templates

The default templates in [github.com/dfinity/icp-cli-templates](https://github.com/dfinity/icp-cli-templates) serve as good
examples to follow.

To use more advanced features of cargo-generate, it is recommended you check out the book [https://cargo-generate.github.io/cargo-generate/](https://cargo-generate.github.io/cargo-generate/).

## Testing Templates

### Local Testing

Test without publishing:

```bash
# Test from local directory
icp new test-project --path ./my-template

# Verify the generated project
cd test-project
icp network start -d
icp deploy
```

### Validation Checklist

Before publishing, verify:

- [ ] `icp new` completes without errors
- [ ] Generated project builds: `icp build`
- [ ] Generated project deploys to the local network: `icp deploy`
- [ ] Variables are substituted correctly
- [ ] Conditional content works as expected
- [ ] README is helpful and accurate

## Publishing Templates

### GitHub Repository

1. Push your template to GitHub
2. Users can reference it directly:

```bash
icp new my-project --git https://github.com/username/my-template
```

### With Tags/Branches

Pin to specific versions:

```bash
# Use a tag
icp new my-project --git https://github.com/user/template --tag v1.0.0

# Use a branch
icp new my-project --git https://github.com/user/template --branch stable
```

### Official Templates

The default templates are in [github.com/dfinity/icp-cli-templates](https://github.com/dfinity/icp-cli-templates). To contribute:

1. Fork the repository
2. Add your template as a subfolder
3. Submit a pull request

## Next Steps

- [Tutorial](../tutorial.md) — Use templates to create projects
- [Creating Recipes](creating-recipes.md) — Create reusable build configurations

[Browse all documentation →](../index.md)
