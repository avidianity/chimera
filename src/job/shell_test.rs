use super::*;

fn argv(requested: Option<&str>, fallback: &str) -> Vec<String> {
    Shell::resolve(requested, fallback)
        .unwrap()
        .command("/tmp/step.sh")
}

#[test]
fn unspecified_shell_uses_bash_without_pipefail() {
    assert_eq!(argv(None, "bash"), ["bash", "-e", "/tmp/step.sh"]);
}

#[test]
fn unspecified_shell_falls_back_to_sh() {
    assert_eq!(argv(None, "sh"), ["sh", "-e", "/tmp/step.sh"]);
}

#[test]
fn blank_shell_counts_as_unspecified() {
    assert_eq!(argv(Some("  "), "bash"), ["bash", "-e", "/tmp/step.sh"]);
}

#[test]
fn explicit_bash_enables_pipefail() {
    assert_eq!(
        argv(Some("bash"), "sh"),
        [
            "bash",
            "--noprofile",
            "--norc",
            "-eo",
            "pipefail",
            "/tmp/step.sh"
        ]
    );
}

#[test]
fn explicit_sh() {
    assert_eq!(argv(Some("sh"), "bash"), ["sh", "-e", "/tmp/step.sh"]);
}

#[test]
fn python_gets_py_extension() {
    let shell = Shell::resolve(Some("python"), "bash").unwrap();

    assert_eq!(shell.command("/tmp/s.py"), ["python", "/tmp/s.py"]);
    assert_eq!(shell.extension(), ".py");
}

#[test]
fn pwsh_dot_sources_the_script() {
    let shell = Shell::resolve(Some("pwsh"), "bash").unwrap();

    assert_eq!(
        shell.command("/tmp/s.ps1"),
        ["pwsh", "-command", ". '/tmp/s.ps1'"]
    );
    assert_eq!(shell.extension(), ".ps1");
}

#[test]
fn pwsh_script_stops_on_error_and_propagates_exit_code() {
    let shell = Shell::resolve(Some("pwsh"), "bash").unwrap();

    let script = shell.prepare_script("Write-Output hi");

    assert!(script.starts_with("$ErrorActionPreference = 'stop'\nWrite-Output hi\n"));
    assert!(script.ends_with("{ exit $LASTEXITCODE }"));
}

#[test]
fn non_powershell_script_is_unchanged() {
    let shell = Shell::resolve(Some("bash"), "bash").unwrap();

    assert_eq!(shell.prepare_script("echo hi"), "echo hi");
}

#[test]
fn custom_shell_substitutes_the_script_path() {
    assert_eq!(
        argv(Some("perl -w {0}"), "bash"),
        ["perl", "-w", "/tmp/step.sh"]
    );
}

#[test]
fn custom_shell_without_placeholder_is_rejected() {
    assert!(Shell::resolve(Some("perl -w"), "bash").is_err());
}

#[test]
fn cmd_is_rejected() {
    assert!(Shell::resolve(Some("cmd"), "bash").is_err());
}
