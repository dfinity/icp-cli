# Recipe Documentation Verification

**IMPORTANT**: This project uses recipes from `icp-cli-recipes` repository. When working with recipe-related documentation or examples:

## Always Verify Against icp-cli-recipes

1. **Check the actual recipe template**: Recipe templates are in `github.com/dfinity/icp-cli-recipes/recipes/<name>/recipe.hbs`
2. **Verify parameters match**: Documentation must match what the recipe template actually supports
3. **Check required vs optional**: Parameters used directly in templates (not in `{{#if}}`) are required

## Documentation Consistency Checklist

When modifying recipe-related docs or examples, verify:

1. **YAML syntax**: Use `canisters: - name:` array syntax, not singular `canister:`
2. **Recipe type format**: Use `@dfinity/<recipe-name>@<version>` (e.g., `@dfinity/rust@v3.0.0`), not just `rust`
3. **Parameter accuracy**: Only document parameters that exist in the recipe template
4. **Config option descriptions**: Each parameter description must accurately describe what it does, verified against the actual behavior in the `recipe.hbs` template
5. **Example accuracy**: Examples in `examples/` directories must use correct recipe syntax
6. **README consistency**: Example READMEs must match their `icp.yaml` files

## Key Files to Keep in Sync

- `docs/guides/using-recipes.md` - Main recipe usage guide
- `docs/migration/from-dfx.md` - Migration examples using recipes
- `docs/guides/creating-templates.md` - Template examples using recipes
- `examples/icp-*-recipe/` - Example projects using recipes

## Cross-Repository Verification

Before finalizing recipe-related changes, ask the user:
- Whether they have a local clone of `icp-cli-recipes` with changes to verify against
- Or whether to fetch the latest recipe templates from the remote repository
- What branch or version to compare with

Then verify documentation matches the recipe templates from the specified source.
