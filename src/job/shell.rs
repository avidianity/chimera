//! Maps a step's `shell:` to the command that runs its script file, following the
//! official runner's templates.

use anyhow::{Result, bail};

const SCRIPT_PLACEHOLDER: &str = "{0}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    argv_template: Vec<String>,
    extension: &'static str,
    is_powershell: bool,
}

impl Shell {
    /// Resolve the requested shell. `fallback` is the shell used when none is
    /// requested: `bash` when the environment has it, `sh` otherwise.
    pub fn resolve(requested: Option<&str>, fallback: &str) -> Result<Self> {
        match requested.map(str::trim).filter(|shell| !shell.is_empty()) {
            // Unlike an explicit `shell: bash`, the default skips pipefail.
            None if fallback == "bash" => Ok(Self::builtin("bash -e {0}", ".sh")),
            None => Ok(Self::builtin("sh -e {0}", ".sh")),
            Some(shell) => Self::named_or_custom(shell),
        }
    }

    fn named_or_custom(shell: &str) -> Result<Self> {
        let resolved = match shell {
            "bash" => Self::builtin("bash --noprofile --norc -eo pipefail {0}", ".sh"),
            "sh" => Self::builtin("sh -e {0}", ".sh"),
            "python" => Self::builtin("python {0}", ".py"),
            "pwsh" => Self::powershell("pwsh"),
            "powershell" => Self::powershell("powershell"),
            "cmd" => bail!("shell 'cmd' is only supported on Windows"),
            custom if custom.contains(SCRIPT_PLACEHOLDER) => Self::builtin(custom, ""),
            other => bail!(
                "invalid shell option '{other}': use bash, sh, python, pwsh, powershell, \
                 or a custom command containing {SCRIPT_PLACEHOLDER}"
            ),
        };
        Ok(resolved)
    }

    fn builtin(template: &str, extension: &'static str) -> Self {
        Self {
            argv_template: template.split_whitespace().map(String::from).collect(),
            extension,
            is_powershell: false,
        }
    }

    fn powershell(program: &str) -> Self {
        Self {
            argv_template: vec![program.into(), "-command".into(), ". '{0}'".into()],
            extension: ".ps1",
            is_powershell: true,
        }
    }

    /// File extension the script file needs, since some interpreters dispatch on it.
    pub fn extension(&self) -> &'static str {
        self.extension
    }

    /// PowerShell neither stops on errors nor exits with a native command's code by
    /// default, so the official runner wraps the script to do both.
    pub fn prepare_script(&self, script: &str) -> String {
        if !self.is_powershell {
            return script.to_string();
        }
        format!(
            "$ErrorActionPreference = 'stop'\n{script}\n\
             if ((Test-Path -LiteralPath variable:\\LASTEXITCODE)) {{ exit $LASTEXITCODE }}"
        )
    }

    /// Full argv that runs the script at `script_path`.
    pub fn command(&self, script_path: &str) -> Vec<String> {
        self.argv_template
            .iter()
            .map(|arg| arg.replace(SCRIPT_PLACEHOLDER, script_path))
            .collect()
    }
}

#[cfg(test)]
#[path = "shell_test.rs"]
mod shell_test;
