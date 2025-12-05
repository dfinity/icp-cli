#[allow(clippy::disallowed_types)]
// In this case we allow PathBuf instead of using icp::prelude::* because
// this is what the crargo generate crate expects
use std::path::PathBuf;

use anyhow::Context;
use cargo_generate::{GenerateArgs, TemplatePath, Vcs, generate};
use clap::Args;
use tracing::debug;

mod heading {
    pub const GIT_PARAMETERS: &str = "Git Parameters";
    pub const TEMPLATE_SELECTION: &str = "Template Selection";
    pub const OUTPUT_PARAMETERS: &str = "Output Parameters";
}

/// Validate the project name
// If the name is not valid, cargo generate will throw an error mentioning that
// the name is invalid for `crate_name` which will seem ambiguous to users
//     let valid_ident = regex::Regex::new(r"^([a-zA-Z][a-zA-Z0-9_-]+)$")?;
// see: https://github.com/cargo-generate/cargo-generate/blob/main/src/interactive.rs#L33
fn validate_name(s: &str) -> Result<String, String> {
    let re = regex::Regex::new(r"^([a-zA-Z][a-zA-Z0-9_-]+)$").unwrap();

    if re.is_match(s) {
        Ok(s.to_string())
    } else {
        Err("The required format is [a-zA-Z][a-zA-Z0-9_-]+".to_string())
    }
}

#[derive(Clone, Debug, Args)]
pub struct IcpGenerateArgs {
    #[command(flatten)]
    pub template_path: IcpTemplatePath,

    /// Directory to create / project name; if the name isn't in kebab-case, it will be converted
    /// to kebab-case unless `--force` is given.
    #[arg(long, short, value_parser = validate_name, help_heading = heading::OUTPUT_PARAMETERS)]
    pub name: String,

    /// Don't convert the project name to kebab-case before creating the directory. Note that
    /// `icp-cli` won't overwrite an existing directory, even if `--force` is given.
    #[arg(long, short, action, help_heading = heading::OUTPUT_PARAMETERS)]
    pub force: bool,

    /// Opposite of verbose, suppresses errors & warning in output
    /// Conflicts with --debug, and requires the use of --continue-on-error
    #[arg(
        long,
        short,
        action,
        requires = "continue_on_error"
    )]
    pub quiet: bool,

    /// Continue if errors in templates are encountered
    #[arg(long, action)]
    pub continue_on_error: bool,

    /// If silent mode is set all variables will be extracted from the template_values_file. If a
    /// value is missing the project generation will fail
    #[arg(long, short, requires("name"), action)]
    pub silent: bool,

    /// Specify the VCS used to initialize the generated template.
    #[arg(long, value_parser, help_heading = heading::OUTPUT_PARAMETERS)]
    pub vcs: Option<Vcs>,

    /// Use a different ssh identity
    #[allow(clippy::disallowed_types)]
    #[arg(short = 'i', long = "identity", value_parser, value_name="IDENTITY", help_heading = heading::GIT_PARAMETERS)]
    pub ssh_identity: Option<PathBuf>,

    /// Use a different gitconfig file, if omitted the usual $HOME/.gitconfig will be used
    #[allow(clippy::disallowed_types)]
    #[arg(long = "gitconfig", value_parser, value_name="GITCONFIG_FILE", help_heading = heading::GIT_PARAMETERS)]
    pub gitconfig: Option<PathBuf>,

    /// Define a value for use during template expansion. E.g `--define foo=bar`
    #[arg(long, short, number_of_values = 1, value_parser, help_heading = heading::OUTPUT_PARAMETERS)]
    pub define: Vec<String>,

    /// Generate the template directly into the current dir. No subfolder will be created and no vcs
    /// is initialized.
    #[arg(long, action, help_heading = heading::OUTPUT_PARAMETERS)]
    pub init: bool,

    /// Generate the template directly at the given path.
    #[allow(clippy::disallowed_types)]
    #[arg(long, value_parser, value_name="PATH", help_heading = heading::OUTPUT_PARAMETERS)]
    pub destination: Option<PathBuf>,

    /// Will enforce a fresh git init on the generated project
    #[arg(long, action, help_heading = heading::OUTPUT_PARAMETERS)]
    pub force_git_init: bool,

    /// Allow the template to overwrite existing files in the destination.
    #[arg(short, long, action, help_heading = heading::OUTPUT_PARAMETERS)]
    pub overwrite: bool,

    /// Skip downloading git submodules (if there are any)
    #[arg(long, action, help_heading = heading::GIT_PARAMETERS)]
    pub skip_submodules: bool,
}

impl Default for IcpGenerateArgs {
    fn default() -> Self {
        Self {
            template_path: IcpTemplatePath::default(),
            name: "".to_string(), // name is a required arg
            force: false,
            quiet: false,
            continue_on_error: false,
            silent: false,
            vcs: None,
            ssh_identity: None,
            gitconfig: None,
            define: Vec::default(),
            init: false,
            destination: None,
            force_git_init: false,
            overwrite: false,
            skip_submodules: false,
        }
    }
}

impl From<IcpGenerateArgs> for GenerateArgs {
    fn from(f: IcpGenerateArgs) -> Self {
        Self {
            template_path: f.template_path.into(),
            name: Some(f.name),
            force: f.force,
            quiet: f.quiet,
            continue_on_error: f.continue_on_error,
            silent: f.silent,
            vcs: f.vcs,
            ssh_identity: f.ssh_identity,
            gitconfig: f.gitconfig,
            define: f.define,
            init: f.init,
            destination: f.destination,
            force_git_init: f.force_git_init,
            overwrite: f.overwrite,
            skip_submodules: f.skip_submodules,
            ..GenerateArgs::default()
        }
    }
}

#[derive(Default, Debug, Clone, Args)]
pub struct IcpTemplatePath {
    /// Auto attempt to use as either `--git` or `--favorite`. If either is specified explicitly,
    /// use as subfolder.
    #[arg(required_unless_present_any(&["SpecificPath"]))]
    pub auto_path: Option<String>,

    /// Specifies the subfolder within the template repository to be used as the actual template.
    #[arg()]
    pub subfolder: Option<String>,

    /// Git repository to clone template from. Can be a URL (like
    /// `https://github.com/dfinity/icp-cli-project-template`), a path (relative or absolute), or an
    /// `owner/repo` abbreviated GitHub URL (like `dfinity/icp-cli-project-template`).
    ///
    /// Note that icp-cli will first attempt to interpret the `owner/repo` form as a
    /// relative path and only try a GitHub URL if the local path doesn't exist.
    #[arg(short, long, group("SpecificPath"), help_heading = heading::TEMPLATE_SELECTION)]
    pub git: Option<String>,

    /// Branch to use when installing from git
    #[arg(short, long, conflicts_with_all = ["revision", "tag"], help_heading = heading::GIT_PARAMETERS)]
    pub branch: Option<String>,

    /// Tag to use when installing from git
    #[arg(short, long, conflicts_with_all = ["revision", "branch"], help_heading = heading::GIT_PARAMETERS)]
    pub tag: Option<String>,

    /// Git revision to use when installing from git (e.g. a commit hash)
    #[arg(short, long, conflicts_with_all = ["tag", "branch"], alias = "rev", help_heading = heading::GIT_PARAMETERS)]
    pub revision: Option<String>,

    /// Local path to copy the template from. Can not be specified together with --git.
    #[arg(short, long, group("SpecificPath"), help_heading = heading::TEMPLATE_SELECTION)]
    pub path: Option<String>,

    /// Generate a favorite template as defined in the config. In case the favorite is undefined,
    /// use in place of the `--git` option, otherwise specifies the subfolder
    #[arg(long, group("SpecificPath"), help_heading = heading::TEMPLATE_SELECTION)]
    pub favorite: Option<String>,
}

impl From<IcpTemplatePath> for TemplatePath {
    fn from(f: IcpTemplatePath) -> Self {
        Self {
            auto_path: f.auto_path,
            subfolder: f.subfolder,
            git: f.git,
            branch: f.branch,
            tag: f.tag,
            revision: f.revision,
            path: f.path,
            favorite: f.favorite,
            ..TemplatePath::default()
        }
    }
}

pub(crate) async fn exec(
    ctx: &icp::context::Context,
    args: &IcpGenerateArgs,
) -> Result<(), anyhow::Error> {
    // Check for conflicting flags: --quiet and --debug cannot be used together
    // Clap has trouble checking for conflicts because the --debug flag is global
    if args.quiet && ctx.debug {
        anyhow::bail!("--quiet and --debug cannot be used together");
    }

    let mut generate_args: GenerateArgs = args.clone().into();
    generate_args.verbose = ctx.debug; // There is a global --debug flag
    let generate_args = generate_args.clone();

    debug!("Generating project with {generate_args:#?}");

    generate(generate_args).context("Error generating project")?;

    Ok(())
}
