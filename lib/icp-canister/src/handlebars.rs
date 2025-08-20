use std::collections::HashMap;

use handlebars::*;

use crate::{
    BuildSteps, CanisterInstructions, Recipe, SyncSteps,
    manifest::RecipeType,
    recipe::{HandlebarsError, Resolve, ResolveError},
};

pub struct Handlebars {
    pub recipes: HashMap<String, String>,
}

impl Resolve for Handlebars {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        // Sanity check recipe type
        let recipe_type = match &recipe.recipe_type {
            RecipeType::Unknown(typ) => typ.to_owned(),
            _ => panic!("expected unknown recipe"),
        };

        // Search for recipe template
        let tmpl = self
            .recipes
            .iter()
            .find_map(|(name, tmpl)| {
                if name == &recipe_type {
                    Some(tmpl.to_owned())
                } else {
                    None
                }
            })
            .ok_or(ResolveError::Handlebars {
                source: HandlebarsError::Unknown {
                    recipe: recipe_type.to_owned(),
                },
            })?;

        // Load the template via handlebars
        let mut reg = handlebars::Handlebars::new();

        // Register helpers
        reg.register_helper("replace", Box::new(ReplaceHelper));

        // Register partials
        for (name, partial) in PARTIALS {
            reg.register_partial(name, partial)
                .map_err(|err| ResolveError::Handlebars {
                    source: HandlebarsError::PartialInvalid {
                        source: err,
                        partial: name.to_owned(),
                        template: partial.to_owned(),
                    },
                })?;
        }

        // Reject unset template variables
        reg.set_strict_mode(true);

        // Render the template to YAML
        let out = reg
            .render_template(&tmpl, &recipe.instructions)
            .map_err(|err| ResolveError::Handlebars {
                source: HandlebarsError::Render {
                    source: err,
                    recipe: recipe_type.to_owned(),
                    template: tmpl.to_owned(),
                },
            })?;

        // Read the rendered YAML canister manifest
        let insts = serde_yaml::from_str::<CanisterInstructions>(&out).unwrap();

        let (build, sync) = match insts {
            // Supported
            CanisterInstructions::BuildSync { build, sync } => (build, sync),

            // Unsupported
            CanisterInstructions::Recipe { .. } => {
                panic!("recipe within a recipe is not supported")
            }
        };

        Ok((build, sync))
    }
}

#[derive(Clone, Copy)]
struct ReplaceHelper;

impl HelperDef for ReplaceHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &'reg handlebars::Handlebars<'reg>,
        _: &Context,
        _: &mut handlebars::RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let (from, to) = (
            h.param(0).unwrap().render(), // from
            h.param(1).unwrap().render(), // to
        );

        let v = h.param(2).unwrap().render();
        out.write(&v.replace(&from, &to))?;

        Ok(())
    }
}

pub const WASM_SHRINK_PARTIAL: &str = r#"
- type: script
  commands:
    - sh -c 'command -v ic-wasm >/dev/null 2>&1 || { echo >&2 "ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n"; exit 1; }'
    - sh -c 'ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" shrink --keep-name-section'
"#;

pub const WASM_COMPRESS_PARTIAL: &str = r#"
- type: script
  commands:
    - sh -c 'command -v gzip >/dev/null 2>&1 || { echo >&2 "gzip not found. Please install gzip to compress build output. \n"; exit 1; }'
    - sh -c 'gzip --no-name "$ICP_WASM_OUTPUT_PATH"'
    - sh -c 'mv "${ICP_WASM_OUTPUT_PATH}.gz" "$ICP_WASM_OUTPUT_PATH"'
"#;

pub const WASM_OPTIMIZE_PARTIAL: &str = r#"
{{> wasm-shrink }}
{{> wasm-compress }}
"#;

pub const WASM_INJECT_METADATA_PARTIAL: &str = r#"
- type: script
  commands:
    - sh -c 'command -v ic-wasm >/dev/null 2>&1 || { echo >&2 "ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n"; exit 1; }'
    - sh -c 'ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "{{ name }}" -d "{{ value }}" --keep-name-section'
"#;

pub const PARTIALS: [(&str, &str); 4] = [
    ("wasm-shrink", WASM_SHRINK_PARTIAL),
    ("wasm-compress", WASM_COMPRESS_PARTIAL),
    ("wasm-optimize", WASM_OPTIMIZE_PARTIAL),
    ("wasm-inject-metadata", WASM_INJECT_METADATA_PARTIAL),
];

pub const PREBUILT_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: pre-built
      path: {{ path }}
      sha256: {{ sha256 }}

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata }}
    {{/each}}
    {{/if}}
"#;

pub const ASSETS_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: pre-built
      url: https://github.com/dfinity/sdk/raw/refs/tags/{{ version }}/src/distributed/assetstorage.wasm.gz

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata }}
    {{/each}}
    {{/if}}

sync:
  steps:
    - type: assets
      dir: {{ dir }}
"#;

pub const MOTOKO_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: script
      commands:
        - sh -c 'command -v moc >/dev/null 2>&1 || { echo >&2 "moc not found. To install moc, see https://internetcomputer.org/docs/building-apps/getting-started/install \n"; exit 1; }'
        - sh -c 'moc {{ entry }}'
        - sh -c 'mv main.wasm "$ICP_WASM_OUTPUT_PATH"'

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata }}
    {{/each}}
    {{/if}}
"#;

pub const RUST_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: script
      commands:
        - cargo build --package {{ package }} --target wasm32-unknown-unknown --release
        - sh -c 'mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" package }}.wasm "$ICP_WASM_OUTPUT_PATH"'

    {{#if shrink }}
    {{> wasm-shrink }}
    {{/if}}

    {{#if compress }}
    {{> wasm-compress }}
    {{/if}}

    {{#if metadata }}
    {{#each metadata }}
    {{> wasm-inject-metadata }}
    {{/each}}
    {{/if}}
"#;

pub const TEMPLATES: [(&str, &str); 4] = [
    ("prebuilt", PREBUILT_CANISTER_TEMPLATE),
    ("handlebars-assets", ASSETS_CANISTER_TEMPLATE),
    ("handlebars-motoko", MOTOKO_CANISTER_TEMPLATE),
    ("handlebars-rust", RUST_CANISTER_TEMPLATE),
];
