use super::*;

fn registration() -> StartupRegistration {
    StartupRegistration {
        executable: PathBuf::from("/Applications/rsproxy & tools/rsproxy"),
        arguments: vec!["startup".to_string(), "launch".to_string()],
    }
}

#[test]
fn macos_launch_agent_escapes_program_arguments() {
    let rendered = render_macos_launch_agent(&registration());
    assert!(rendered.contains("dev.rsproxy.autostart"));
    assert!(rendered.contains("/Applications/rsproxy &amp; tools/rsproxy"));
    assert!(rendered.contains("<key>RunAtLoad</key>"));
}

#[test]
fn linux_desktop_entry_quotes_exec_arguments() {
    let rendered = render_linux_desktop_entry(&registration());
    assert!(
        rendered.contains("Exec=\"/Applications/rsproxy & tools/rsproxy\" \"startup\" \"launch\"")
    );
    assert!(rendered.contains("X-GNOME-Autostart-enabled=true"));
}

#[test]
fn linux_desktop_entry_doubles_backslashes_for_the_keyfile_layer() {
    let registration = StartupRegistration {
        executable: PathBuf::from(r"/opt/$cache/rs\proxy"),
        arguments: vec!["startup".to_string()],
    };
    let rendered = render_linux_desktop_entry(&registration);
    assert!(rendered.contains(r#"Exec="/opt/\\$cache/rs\\\\proxy" "startup""#));
}

#[test]
fn windows_command_line_preserves_spaces_and_trailing_backslashes() {
    let registration = StartupRegistration {
        executable: PathBuf::from(r"C:\Program Files\rsproxy\rsproxy.exe"),
        arguments: vec!["startup".to_string(), r"C:\state\".to_string()],
    };
    let rendered = windows_command_line(&registration);
    assert_eq!(
        rendered,
        r#""C:\Program Files\rsproxy\rsproxy.exe" "startup" "C:\state\\""#
    );
}

#[test]
fn registration_rejects_relative_executable_paths() {
    let error = validate_registration(&StartupRegistration {
        executable: PathBuf::from("rsproxy"),
        arguments: vec![],
    })
    .unwrap_err();
    assert!(error.to_string().contains("must be absolute"));
}
