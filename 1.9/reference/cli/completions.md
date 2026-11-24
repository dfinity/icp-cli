# icp completions

Generate a shell completion script

The script is written to stdout. Save it where your shell loads completions from, or source it from your shell profile.

**Usage:** `icp completions <SHELL>`

Examples:

    # Bash
    icp completions bash > ~/.local/share/bash-completion/completions/icp

    # Zsh, into a directory on your $fpath
    icp completions zsh > ~/.zfunc/_icp

    # Fish
    icp completions fish > ~/.config/fish/completions/icp.fish

    # Elvish, appended to your profile
    icp completions elvish >> ~/.elvish/rc.elv

    # PowerShell, appended to your profile
    icp completions powershell >> $PROFILE


###### **Arguments:**

* `<SHELL>` — The shell to generate a completion script for

  Possible values: `bash`, `elvish`, `fish`, `powershell`, `zsh`





