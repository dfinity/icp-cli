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

pub const ASSETS_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: pre-built
      url: https://github.com/dfinity/sdk/raw/refs/tags/{{ version }}/src/distributed/assetstorage.wasm.gz

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
"#;

pub const RUST_CANISTER_TEMPLATE: &str = r#"
build:
  steps:
    - type: script
      commands:
        - cargo build --package {{ package }} --target wasm32-unknown-unknown --release
        - sh -c 'mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" package }}.wasm "$ICP_WASM_OUTPUT_PATH"'
"#;

pub const TEMPLATES: [(&str, &str); 3] = [
    ("handlebars-assets", ASSETS_CANISTER_TEMPLATE),
    ("handlebars-motoko", MOTOKO_CANISTER_TEMPLATE),
    ("handlebars-rust", RUST_CANISTER_TEMPLATE),
];
