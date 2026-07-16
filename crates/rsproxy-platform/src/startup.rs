use crate::{PlatformError, PlatformResult};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

const STARTUP_ID: &str = "dev.rsproxy.autostart";
const WINDOWS_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const WINDOWS_VALUE_NAME: &str = "rsproxy";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Native per-user login-startup backend selected for the current operating system.
pub enum StartupPlatform {
    /// A macOS LaunchAgent below the current user's Library directory.
    Macos,
    /// A value in the current user's Windows `Run` registry key.
    Windows,
    /// A freedesktop XDG Autostart desktop entry.
    Linux,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Executable and arguments registered to run when the current user logs in.
pub struct StartupRegistration {
    /// Absolute path to the rsproxy executable.
    pub executable: PathBuf,
    /// Arguments passed to the executable by the desktop startup backend.
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Current state of the per-user login-startup registration.
pub struct StartupStatus {
    /// Platform backend used for registration.
    pub platform: StartupPlatform,
    /// Whether the rsproxy startup entry currently exists.
    pub installed: bool,
    /// Human-readable artifact or registry location.
    pub location: String,
}

/// Returns the login-startup backend for this build target.
pub const fn current_startup_platform() -> StartupPlatform {
    if cfg!(target_os = "macos") {
        StartupPlatform::Macos
    } else if cfg!(windows) {
        StartupPlatform::Windows
    } else {
        StartupPlatform::Linux
    }
}

/// Returns the stable per-user manifest path consumed by `rsproxy startup launch`.
pub fn startup_manifest_path() -> PlatformResult<PathBuf> {
    let home = user_home()?;
    Ok(match current_startup_platform() {
        StartupPlatform::Macos => home
            .join("Library")
            .join("Application Support")
            .join("rsproxy")
            .join("startup.json"),
        StartupPlatform::Windows => windows_config_home(&home)
            .join("rsproxy")
            .join("startup.json"),
        StartupPlatform::Linux => xdg_config_home(&home).join("rsproxy").join("startup.json"),
    })
}

/// Installs or replaces the current user's login-startup registration.
pub fn install_startup(registration: &StartupRegistration) -> PlatformResult<StartupStatus> {
    validate_registration(registration)?;
    let platform = current_startup_platform();
    let location = match file_backend(platform)? {
        Some(backend) => {
            install_file_registration(
                &backend.path,
                (backend.render)(registration).as_bytes(),
                backend.write_context,
            )?;
            backend.path.display().to_string()
        }
        None => {
            install_windows_registration(registration)?;
            windows_location()
        }
    };
    Ok(StartupStatus {
        platform,
        installed: true,
        location,
    })
}

/// Removes the current user's login-startup registration if it exists.
pub fn uninstall_startup() -> PlatformResult<StartupStatus> {
    let platform = current_startup_platform();
    let location = match file_backend(platform)? {
        Some(backend) => {
            remove_file_if_exists(&backend.path, backend.remove_context)?;
            backend.path.display().to_string()
        }
        None => {
            uninstall_windows_registration()?;
            windows_location()
        }
    };
    Ok(StartupStatus {
        platform,
        installed: false,
        location,
    })
}

/// Atomically writes the per-user startup launcher manifest with owner-only permissions.
pub fn write_startup_manifest(contents: &[u8]) -> PlatformResult<()> {
    install_file_registration(
        &startup_manifest_path()?,
        contents,
        "write startup manifest",
    )
}

/// Removes the per-user startup launcher manifest if it exists.
pub fn remove_startup_manifest() -> PlatformResult<()> {
    remove_file_if_exists(&startup_manifest_path()?, "remove startup manifest")
}

/// Inspects whether the current user's login-startup registration exists.
pub fn startup_status() -> PlatformResult<StartupStatus> {
    let platform = current_startup_platform();
    match file_backend(platform)? {
        Some(backend) => Ok(StartupStatus {
            platform,
            installed: backend.path.is_file(),
            location: backend.path.display().to_string(),
        }),
        None => Ok(StartupStatus {
            platform,
            installed: windows_registration_exists()?,
            location: windows_location(),
        }),
    }
}

/// File-based login entry shared by the macOS and Linux backends; Windows uses the registry.
struct FileBackend {
    path: PathBuf,
    render: fn(&StartupRegistration) -> String,
    write_context: &'static str,
    remove_context: &'static str,
}

fn file_backend(platform: StartupPlatform) -> PlatformResult<Option<FileBackend>> {
    Ok(match platform {
        StartupPlatform::Macos => Some(FileBackend {
            path: macos_launch_agent_path(&user_home()?),
            render: render_macos_launch_agent,
            write_context: "write macOS LaunchAgent",
            remove_context: "remove macOS LaunchAgent",
        }),
        StartupPlatform::Linux => Some(FileBackend {
            path: linux_desktop_entry_path(&user_home()?),
            render: render_linux_desktop_entry,
            write_context: "write Linux autostart entry",
            remove_context: "remove Linux autostart entry",
        }),
        StartupPlatform::Windows => None,
    })
}

fn windows_location() -> String {
    format!(r"{WINDOWS_RUN_KEY}\{WINDOWS_VALUE_NAME}")
}

fn validate_registration(registration: &StartupRegistration) -> PlatformResult<()> {
    if !registration.executable.is_absolute() {
        return Err(PlatformError::InvalidState(
            "startup executable path must be absolute".to_string(),
        ));
    }
    let executable = registration.executable.to_string_lossy();
    if executable.contains(['\0', '\n', '\r'])
        || registration
            .arguments
            .iter()
            .any(|argument| argument.contains(['\0', '\n', '\r']))
    {
        return Err(PlatformError::InvalidState(
            "startup executable and arguments must not contain NUL or newline characters"
                .to_string(),
        ));
    }
    Ok(())
}

fn user_home() -> PlatformResult<PathBuf> {
    // A set-but-unusable candidate (e.g. a POSIX-style HOME exported by Git Bash on Windows)
    // must not shadow a later absolute one, so filter per candidate rather than per chain.
    [
        env::var_os("HOME"),
        env::var_os("USERPROFILE"),
        windows_home_from_parts(),
    ]
    .into_iter()
    .flatten()
    .map(PathBuf::from)
    .find(|path| path.is_absolute())
    .ok_or_else(|| {
        PlatformError::InvalidState(
            "cannot resolve an absolute current-user home directory".to_string(),
        )
    })
}

fn windows_home_from_parts() -> Option<OsString> {
    let drive = env::var_os("HOMEDRIVE")?;
    let path = env::var_os("HOMEPATH")?;
    let mut home = drive;
    home.push(path);
    Some(home)
}

fn windows_config_home(home: &Path) -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
}

fn xdg_config_home(home: &Path) -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
}

fn macos_launch_agent_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{STARTUP_ID}.plist"))
}

fn linux_desktop_entry_path(home: &Path) -> PathBuf {
    xdg_config_home(home)
        .join("autostart")
        .join("rsproxy.desktop")
}

fn install_file_registration(path: &Path, contents: &[u8], context: &str) -> PlatformResult<()> {
    let parent = path.parent().ok_or_else(|| {
        PlatformError::InvalidState(format!("startup path {} has no parent", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|source| PlatformError::Io {
        context: format!("create startup directory {}", parent.display()),
        source,
    })?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents).map_err(|source| PlatformError::Io {
        context: format!("{context} temporary file {}", temporary.display()),
        source,
    })?;
    set_owner_only_permissions(&temporary)?;
    replace_file(&temporary, path).map_err(|source| PlatformError::Io {
        context: format!("{context} {}", path.display()),
        source,
    })
}

fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    // std::fs::rename replaces an existing destination file on all supported platforms
    // (MOVEFILE_REPLACE_EXISTING on Windows), so a delete-then-rename window is never needed.
    fs::rename(source, destination)
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> PlatformResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        PlatformError::Io {
            context: format!("set startup file permissions {}", path.display()),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> PlatformResult<()> {
    Ok(())
}

fn remove_file_if_exists(path: &Path, context: &str) -> PlatformResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PlatformError::Io {
            context: format!("{context} {}", path.display()),
            source,
        }),
    }
}

fn argv(registration: &StartupRegistration) -> impl Iterator<Item = String> + '_ {
    std::iter::once(registration.executable.to_string_lossy().into_owned())
        .chain(registration.arguments.iter().cloned())
}

fn render_macos_launch_agent(registration: &StartupRegistration) -> String {
    let arguments = argv(registration)
        .map(|argument| format!("        <string>{}</string>", xml_escape(&argument)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
    <key>Label</key>\n\
    <string>{STARTUP_ID}</string>\n\
    <key>ProgramArguments</key>\n\
    <array>\n{arguments}\n    </array>\n\
    <key>RunAtLoad</key>\n\
    <true/>\n\
    <key>ProcessType</key>\n\
    <string>Background</string>\n\
</dict>\n\
</plist>\n"
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn render_linux_desktop_entry(registration: &StartupRegistration) -> String {
    // The key-file string-escape layer is unescaped before Exec quoting, so every backslash
    // produced by the quoting layer must itself be doubled in the file (a literal `$` inside a
    // quoted argument is written `\\$`, a literal backslash as four backslashes).
    let command = keyfile_escape(
        &argv(registration)
            .map(|argument| desktop_exec_argument(&argument))
            .collect::<Vec<_>>()
            .join(" "),
    );
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Version=1.0\n\
Name=rsproxy\n\
Comment=Start rsproxy and restore system proxy routing\n\
Exec={command}\n\
Terminal=false\n\
NoDisplay=true\n\
X-GNOME-Autostart-enabled=true\n"
    )
}

fn desktop_exec_argument(value: &str) -> String {
    let escaped = value
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

fn keyfile_escape(value: &str) -> String {
    value.replace('\\', "\\\\")
}

#[cfg(any(windows, test))]
fn windows_command_line(registration: &StartupRegistration) -> String {
    argv(registration)
        .map(|argument| windows_quote_argument(&argument))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(windows, test))]
fn windows_quote_argument(value: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
        } else if character == '"' {
            quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
            quoted.push('"');
            backslashes = 0;
        } else {
            quoted.push_str(&"\\".repeat(backslashes));
            backslashes = 0;
            quoted.push(character);
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(windows)]
fn install_windows_registration(registration: &StartupRegistration) -> PlatformResult<()> {
    run_registry_command(&[
        "add".to_string(),
        WINDOWS_RUN_KEY.to_string(),
        "/v".to_string(),
        WINDOWS_VALUE_NAME.to_string(),
        "/t".to_string(),
        "REG_SZ".to_string(),
        "/d".to_string(),
        windows_command_line(registration),
        "/f".to_string(),
    ])
}

#[cfg(not(windows))]
fn install_windows_registration(_registration: &StartupRegistration) -> PlatformResult<()> {
    Err(PlatformError::Unsupported(
        "Windows startup registration requires a Windows build".to_string(),
    ))
}

#[cfg(windows)]
fn uninstall_windows_registration() -> PlatformResult<()> {
    if !windows_registration_exists()? {
        return Ok(());
    }
    run_registry_command(&[
        "delete".to_string(),
        WINDOWS_RUN_KEY.to_string(),
        "/v".to_string(),
        WINDOWS_VALUE_NAME.to_string(),
        "/f".to_string(),
    ])
}

#[cfg(not(windows))]
fn uninstall_windows_registration() -> PlatformResult<()> {
    Err(PlatformError::Unsupported(
        "Windows startup registration requires a Windows build".to_string(),
    ))
}

#[cfg(windows)]
fn windows_registration_exists() -> PlatformResult<bool> {
    let output = Command::new("reg")
        .args(["query", WINDOWS_RUN_KEY, "/v", WINDOWS_VALUE_NAME])
        .output()
        .map_err(|source| PlatformError::Io {
            context: "query Windows startup registration".to_string(),
            source,
        })?;
    Ok(output.status.success())
}

#[cfg(not(windows))]
fn windows_registration_exists() -> PlatformResult<bool> {
    Err(PlatformError::Unsupported(
        "Windows startup registration requires a Windows build".to_string(),
    ))
}

#[cfg(windows)]
fn run_registry_command(args: &[String]) -> PlatformResult<()> {
    crate::system_proxy::platform_command_output("reg", args).map(|_| ())
}

#[cfg(test)]
mod tests;
