# icp new

Create a new ICP project from a template

Under the hood templates are generated with `cargo-generate`. See the cargo-generate docs for a guide on how to write your own templates: https://docs.rs/cargo-generate/0.23.7/cargo_generate/

**Usage:** `icp new [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Directory to create / project name; if the name isn't in kebab-case, it will be converted to kebab-case unless `--force` is given

###### **Options:**

* `--subfolder <SUBFOLDER>` — Specifies the subfolder within the template repository to be used as the actual template
* `-g`, `--git <GIT>` — Git repository to clone template from. Can be a URL (like `https://github.com/dfinity/icp-cli-project-template`), a path (relative or absolute)

  Default value: `https://github.com/dfinity/icp-cli-templates`
* `-b`, `--branch <BRANCH>` — Branch to use when installing from git
* `-t`, `--tag <TAG>` — Tag to use when installing from git
* `-r`, `--revision <REVISION>` — Git revision to use when installing from git (e.g. a commit hash)
* `-p`, `--path <PATH>` — Local path to copy the template from. Can not be specified together with --git
* `-f`, `--force` — Don't convert the project name to kebab-case before creating the directory. Note that `icp-cli` won't overwrite an existing directory, even if `--force` is given
* `-q`, `--quiet` — Opposite of verbose, suppresses errors & warning in output Conflicts with --debug, and requires the use of --continue-on-error
* `--continue-on-error` — Continue if errors in templates are encountered
* `-s`, `--silent` — If silent mode is set all variables will be extracted from the template_values_file. If a value is missing the project generation will fail
* `--vcs <VCS>` — Specify the VCS used to initialize the generated template
* `-i`, `--identity <IDENTITY>` — Use a different ssh identity
* `--gitconfig <GITCONFIG_FILE>` — Use a different gitconfig file, if omitted the usual $HOME/.gitconfig will be used
* `-d`, `--define <DEFINE>` — Define a value for use during template expansion. E.g `--define foo=bar`
* `--init` — Generate the template directly into the current dir. No subfolder will be created and no vcs is initialized
* `--destination <PATH>` — Generate the template directly at the given path
* `--force-git-init` — Will enforce a fresh git init on the generated project
* `-o`, `--overwrite` — Allow the template to overwrite existing files in the destination
* `--skip-submodules` — Skip downloading git submodules (if there are any)




