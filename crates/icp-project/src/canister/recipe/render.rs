//! Stage two of recipe resolution: turn template text into build/sync steps.

use std::collections::HashMap;

use handlebars::{Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext};
use serde::Deserialize;
use snafu::prelude::*;
use tracing::debug;

use crate::manifest::{
    canister::{BuildSteps, SyncSteps},
    recipe::{Recipe, RecipeType},
};

/// Describes the canister being built, for the render stage.
///
/// Belongs to rendering alone: [`Resolve::resolve`](super::Resolve::resolve) no
/// longer takes it, since fetching a template does not depend on which canister
/// the template is for. Only [`render_recipe`] consumes it.
///
/// Serializes to the shape injected into recipe templates under the `_` namespace:
///
/// ```yaml
/// canister:
///   name: <canister_name>
/// ```
pub struct RecipeContext {
    pub canister_name: String,
}

impl RecipeContext {
    /// Builds the YAML value injected into recipe templates under the `_` namespace.
    /// Constructing the mapping directly is infallible, unlike `serde` serialization.
    pub fn to_yaml(&self) -> serde_yaml::Value {
        use serde_yaml::{Mapping, Value};

        let mut canister = Mapping::new();
        canister.insert("name".into(), Value::String(self.canister_name.clone()));

        let mut root = Mapping::new();
        root.insert("canister".into(), Value::Mapping(canister));

        Value::Mapping(root)
    }
}

#[derive(Debug, Snafu)]
pub enum RenderRecipeError {
    #[snafu(display("recipe template for '{recipe}' failed to render"))]
    Render {
        // Boxed to keep `Result<_, RenderRecipeError>` small; `RenderError`
        // alone is well over a hundred bytes.
        #[snafu(source(from(handlebars::RenderError, Box::new)))]
        source: Box<handlebars::RenderError>,
        recipe: RecipeType,
    },

    #[snafu(display("recipe '{recipe}' did not render into a valid build/sync manifest"))]
    Parse {
        source: serde_yaml::Error,
        recipe: RecipeType,
    },
}

/// Render a recipe's Handlebars `template` into concrete build/sync steps.
///
/// The template is rendered with the recipe's `configuration` plus the reserved
/// `_` namespace (the `_` key always overrides any user-supplied value), then the
/// resulting YAML is parsed. A recipe may only produce `build` and `sync`.
pub fn render_recipe(
    template: &str,
    recipe: &Recipe,
    recipe_context: &RecipeContext,
) -> Result<(BuildSteps, SyncSteps), RenderRecipeError> {
    let mut reg = Handlebars::new();
    // The output is YAML, not HTML, so disable HTML escaping.
    reg.register_escape_fn(handlebars::no_escape);
    reg.register_helper("replace", Box::new(ReplaceHelper));
    // Reject unset template variables.
    reg.set_strict_mode(true);

    // User-provided configuration plus the injected `_.*` variables. The `_` key
    // is reserved and always overrides any user-supplied value.
    let mut render_context: HashMap<String, serde_yaml::Value> = recipe.configuration.clone();
    render_context.insert("_".to_string(), recipe_context.to_yaml());

    debug!("Rendering recipe template:\n------\n{template}\n------");

    let out = reg
        .render_template(template, &render_context)
        .context(RenderSnafu {
            recipe: recipe.recipe_type.clone(),
        })?;

    // Logged rather than carried in `Parse` below: a recipe author debugging a
    // malformed render needs the whole document, which is too much for an error
    // message.
    debug!("Rendered recipe template:\n------\n{out}\n------");

    // Recipes can only render `build`/`sync`.
    #[derive(Deserialize)]
    struct BuildSyncHelper {
        build: BuildSteps,
        #[serde(default)]
        sync: SyncSteps,
    }

    let helper: BuildSyncHelper = serde_yaml::from_str(&out).context(ParseSnafu {
        recipe: recipe.recipe_type.clone(),
    })?;
    Ok((helper.build, helper.sync))
}

/// Handlebars helper for string replacement operations.
/// Usage: `{{ replace "from" "to" value }}`
#[derive(Clone, Copy)]
struct ReplaceHelper;

impl HelperDef for ReplaceHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &'reg Handlebars<'reg>,
        _: &Context,
        _: &mut RenderContext<'reg, 'rc>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::canister::BuildStep;

    fn recipe(config: &[(&str, &str)]) -> Recipe {
        Recipe {
            recipe_type: RecipeType::File("recipe.hbs".to_owned()),
            configuration: config
                .iter()
                .map(|(k, v)| ((*k).to_owned(), serde_yaml::Value::String((*v).to_owned())))
                .collect(),
            sha256: None,
        }
    }

    fn ctx(name: &str) -> RecipeContext {
        RecipeContext {
            canister_name: name.to_owned(),
        }
    }

    /// The only build step's command, for a recipe that renders a single script step.
    fn rendered_command(template: &str, recipe: &Recipe, context: &RecipeContext) -> String {
        let (build, _sync) = render_recipe(template, recipe, context).unwrap();
        match &build.steps[0] {
            BuildStep::Script(adapter) => adapter.command.as_vec()[0].clone(),
            other => panic!("expected a script build step, got {other:?}"),
        }
    }

    /// Interpolated values are not HTML-escaped (the output is YAML): `=` and `&`
    /// must survive.
    #[test]
    fn template_values_are_not_html_escaped() {
        let template = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "{{ command }}"
        "#};
        let r = recipe(&[("command", "SITE=https://example.com&foo=bar npm run build")]);
        assert_eq!(
            rendered_command(template, &r, &ctx("my-canister")),
            "SITE=https://example.com&foo=bar npm run build"
        );
    }

    /// The canister name is injected under the reserved `_` namespace.
    #[test]
    fn canister_name_is_injected() {
        let template = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "build {{_.canister.name}}"
        "#};
        assert_eq!(
            rendered_command(template, &recipe(&[]), &ctx("my-canister")),
            "build my-canister"
        );
    }

    /// The `_` namespace works through the `replace` helper.
    #[test]
    fn canister_name_works_with_replace_helper() {
        let template = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "cp {{ replace "-" "_" _.canister.name }}.wasm out.wasm"
        "#};
        assert_eq!(
            rendered_command(template, &recipe(&[]), &ctx("my-canister")),
            "cp my_canister.wasm out.wasm"
        );
    }

    /// User configuration cannot override the reserved `_` namespace.
    #[test]
    fn reserved_namespace_cannot_be_overridden_by_user_config() {
        let template = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "build {{_.canister.name}}"
        "#};
        let mut r = recipe(&[]);
        r.configuration.insert(
            "_".to_owned(),
            serde_yaml::from_str("canister:\n  name: user-override").unwrap(),
        );
        assert_eq!(
            rendered_command(template, &r, &ctx("real-name")),
            "build real-name"
        );
    }

    /// A template referencing an unset variable is a `Render` error, because
    /// strict mode is on.
    #[test]
    fn unset_template_variable_is_a_render_error() {
        let template = indoc::indoc! {r#"
            build:
              steps:
                - type: script
                  command: "{{ never_set }}"
        "#};
        assert!(matches!(
            render_recipe(template, &recipe(&[]), &ctx("c")),
            Err(RenderRecipeError::Render { .. })
        ));
    }

    /// A template that renders to invalid build/sync YAML is a `Parse` error,
    /// not a panic.
    #[test]
    fn invalid_rendered_yaml_is_a_parse_error() {
        let template = "not: a valid build manifest\n";
        assert!(matches!(
            render_recipe(template, &recipe(&[]), &ctx("c")),
            Err(RenderRecipeError::Parse { .. })
        ));
    }
}
