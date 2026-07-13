use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn completion_scripts_cover_supported_shells_without_runtime_side_effects() {
    let storage = unique_temp_dir();
    for (shell, marker) in [
        ("bash", "complete -F _rsproxy"),
        ("zsh", "#compdef rsproxy"),
        ("fish", "complete -c rsproxy"),
        ("powershell", "Register-ArgumentCompleter"),
        ("pwsh", "Register-ArgumentCompleter"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(marker));
        assert!(stdout.contains("rules"));
        assert!(stdout.contains("trace"));
    }
    assert!(!storage.exists());
}

#[test]
fn completion_errors_are_explicit() {
    for args in [&["completions"][..], &["completions", "unknown-shell"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("<SHELL>") || stderr.contains("invalid value"));
        assert!(stderr.contains("--help"));
    }
}

fn unique_temp_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rsproxy-completions-{}-{nonce}",
        std::process::id()
    ))
}
