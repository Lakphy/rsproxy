//! Behavioral smoke tests for the platform facade.
#![allow(clippy::unwrap_used)]

use rsproxy_platform::ca::{
    CaInitialization, TrustOptions, generate_root_ca, initialize_root_ca, install_root_ca,
    read_root_ca, root_ca_status,
};
use rsproxy_platform::process::{
    detach_daemon, force_terminate_process, parse_pid, process_alive, resident_kib,
    terminate_process,
};
use rsproxy_platform::system_proxy::{
    ProxyAction, ProxyOptions, ProxyOutcome, ProxyPlanStep, ProxyPlatform, ProxyTarget,
    execute_system_proxy, plan_system_proxy,
};
use rsproxy_platform::{PlatformError, PlatformResult};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn typed_error_facade_is_public() {
    fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
    fn accept_result(_: PlatformResult<()>) {}

    assert_error::<PlatformError>();
    accept_result(Err(PlatformError::Timeout {
        operation: "public API assertion".to_string(),
        timeout_ms: 10,
        output: String::new(),
    }));
}

#[test]
fn ca_facade_exposes_root_storage_and_trust_plan_without_engine_types() {
    let directory = std::env::temp_dir().join(format!(
        "rsproxy-platform-public-api-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let generated = generate_root_ca("rsproxy platform public API root").unwrap();
    assert!(generated.certificate_pem.contains("BEGIN CERTIFICATE"));

    let initialization =
        initialize_root_ca(&directory, "rsproxy platform public API root", false).unwrap();
    assert!(matches!(initialization, CaInitialization::Created { .. }));
    assert!(root_ca_status(&directory).unwrap().initialized);
    assert!(
        read_root_ca(&directory)
            .unwrap()
            .certificate_pem
            .contains("BEGIN CERTIFICATE")
    );

    let plan = install_root_ca(
        &directory,
        &TrustOptions {
            keychain: Some(directory.join("dry-run.keychain-db")),
            dry_run: true,
        },
    )
    .unwrap();
    assert!(plan.dry_run);
    assert!(!plan.commands.is_empty());
    assert_eq!(
        plan.certificate,
        root_ca_status(&directory).unwrap().paths.certificate
    );

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn process_facade_uses_typed_process_identifiers() {
    assert_eq!(parse_pid("42").unwrap(), 42);
    assert!(process_alive(std::process::id()));
    let _detach: fn(&mut Command) = detach_daemon;
    let _terminate: fn(u32) -> PlatformResult<()> = terminate_process;
    let _force_terminate: fn(u32) -> PlatformResult<()> = force_terminate_process;
    assert!(resident_kib(std::process::id()).is_some());
}

#[test]
fn system_proxy_facade_accepts_typed_options_and_returns_a_render_neutral_report() {
    let options = ProxyOptions {
        target: Some(ProxyTarget {
            host: "127.0.0.1".to_string(),
            port: 18916,
        }),
        bypass: Some(vec!["localhost".to_string()]),
        service: None,
        all_services: false,
    };
    let plan = plan_system_proxy(ProxyPlatform::Windows, ProxyAction::Enable, &options).unwrap();
    assert_eq!(plan.platform, ProxyPlatform::Windows);
    assert!(
        plan.steps
            .iter()
            .any(|step| matches!(step, ProxyPlanStep::Command(_)))
    );
    assert!(
        plan.steps
            .iter()
            .any(|step| matches!(step, ProxyPlanStep::Change(_)))
    );
    let _execute: fn(ProxyPlatform, ProxyAction, &ProxyOptions) -> PlatformResult<ProxyOutcome> =
        execute_system_proxy;
}

#[cfg(unix)]
#[test]
fn unix_control_socket_assembly_is_public() {
    let storage = std::path::Path::new("/tmp/rsproxy-platform-public-socket");
    assert_eq!(
        rsproxy_platform::process::unix_control_socket_path(storage),
        storage.join("run/ctl.sock")
    );
}
