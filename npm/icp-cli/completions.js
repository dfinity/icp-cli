/**
 * Shell completion installation, run from postinstall.js.
 *
 * Only directories that a shell reads on its own are written to; hooking up a
 * shell that needs a profile edit is left to the user, who can generate the
 * script with `icp completions <shell>`.
 *
 * The generated script does not list commands and flags itself — it loads a
 * hook from the binary, which then answers as the user types. The binary is
 * named by path, so these files are rewritten on every install to follow it.
 */

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

/**
 * The user location bash-completion v2 loads from. `BASH_COMPLETION_USER_DIR`
 * is a colon-separated list searched in order, so its first entry is the one to
 * write to.
 */
function bashCompletionUserDir(home) {
  const configured = (process.env.BASH_COMPLETION_USER_DIR || '')
    .split(':')
    .find((dir) => dir !== '');
  if (configured) {
    return configured;
  }
  const dataHome = process.env.XDG_DATA_HOME || path.join(home, '.local', 'share');
  return path.join(dataHome, 'bash-completion');
}

function completionTargets(home) {
  const fishConfig = path.join(
    process.env.XDG_CONFIG_HOME || path.join(home, '.config'),
    'fish'
  );

  return [
    {
      shell: 'bash',
      dir: path.join(bashCompletionUserDir(home), 'completions'),
      file: 'icp'
    },
    {
      shell: 'fish',
      dir: path.join(fishConfig, 'completions'),
      file: 'icp.fish',
      // Only if fish is actually configured; fish creates this itself otherwise.
      requires: fishConfig
    },
    {
      shell: 'zsh',
      // Not on zsh's default $fpath, so only useful if the user set it up.
      dir: path.join(home, '.zfunc'),
      file: '_icp',
      requires: path.join(home, '.zfunc')
    }
  ];
}

/** Generate a completion script, or throw with what the binary reported. */
function generate(binaryPath, shell) {
  const result = spawnSync(binaryPath, ['completions', shell], {
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    const detail = (result.stderr || '').trim();
    throw new Error(
      `\`icp completions ${shell}\` exited with status ${result.status}` +
        (detail ? `: ${detail}` : '')
    );
  }
  if (!result.stdout) {
    throw new Error(`\`icp completions ${shell}\` produced no output`);
  }
  return result.stdout;
}


/**
 * Install completion scripts for the shells that can pick them up automatically.
 *
 * A failure is reported but does not fail the install: completions are a
 * convenience, and an unwritable home directory is not an installation error.
 *
 * @returns {string[]} the shells whose completions were installed
 */
function installCompletions(binaryPath) {
  if (process.env.ICP_CLI_SKIP_COMPLETIONS || process.platform === 'win32') {
    return [];
  }

  const home = os.homedir();
  if (!home) {
    console.error(
      'WARNING: skipping shell completions: no home directory found\n' +
        '         Once that is resolved, run `icp completions <SHELL>` and save the output ' +
        'where your shell loads completions from.'
    );
    return [];
  }

  const installed = [];
  for (const target of completionTargets(home)) {
    if (target.requires && !fs.existsSync(target.requires)) {
      continue;
    }
    const destination = path.join(target.dir, target.file);
    try {
      const script = generate(binaryPath, target.shell);
      fs.mkdirSync(target.dir, { recursive: true });
      fs.writeFileSync(destination, script, { mode: 0o644 });
      installed.push(target.shell);
    } catch (err) {
      console.error(
        `WARNING: could not install ${target.shell} completions at ${destination}: ${err.message}\n` +
          `         Once that is resolved, run: icp completions ${target.shell} > ${destination}`
      );
    }
  }

  return installed;
}

module.exports = { installCompletions };
