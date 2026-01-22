# Creating Project Templates

Project templates let users scaffold new ICP projects with `icp new`. This guide covers creating custom templates for your team or the community.

icp-cli uses [cargo-generate](https://cargo-generate.github.io/cargo-generate/) for project templating. Templates are folders or git repositories containing:

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
        main: src/main.mo
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

Use them in templates with [Liquid syntax](https://shopify.github.io/liquid/):

```yaml
# icp.yaml
canisters:

  # ... snip snip for brevity ...

  {% if include_frontend %}
  - name: {{project-name}}-frontend
    recipe:
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist
  {% endif %}

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
# Ignore Rust files when Motoko is selected
[conditional.'backend_language == "motoko"']
ignore = ["Cargo.toml", "src/backend/lib.rs"]

# Ignore Motoko files when Rust is selected
[conditional.'backend_language == "rust"']
ignore = ["src/backend/main.mo"]
```

## Advanced Features

### Conditional Content

Use [Liquid](https://shopify.github.io/liquid/) conditionals in any file:

```yaml
# icp.yaml
canisters:
  - name: {{project-name}}
    {% if backend_language == "rust" %}
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: {{crate_name}}
    {% else %}
    recipe:
      type: "@dfinity/motoko"
      configuration:
        main: src/backend/main.mo
    {% endif %}
```

### Conditional Files

Ignore files based on user choices:

```toml
# cargo-generate.toml
# Ignore frontend files when include_frontend is false
[conditional.'!include_frontend']
ignore = ["src/frontend/", "package.json"]
```

### Post-Generation Hooks

Run [Rhai scripts](https://rhai.rs/) after generation:

```toml
[hooks]
post = ["post-generate.rhai"]
```

Example `post-generate.rhai` script:

```rhai
// Rename a directory based on user selection
let backend = variable::get("backend_type");
if backend == "rust" {
    file::rename("rust-backend", "backend");
} else {
    file::rename("motoko-backend", "backend");
}
```

Note: Hooks execute Rhai scripts, not shell commands directly. See the [cargo-generate scripting documentation](https://cargo-generate.github.io/cargo-generate/templates/scripting.html) for available functions.

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
