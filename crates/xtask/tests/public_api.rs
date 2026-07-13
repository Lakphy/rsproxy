use std::path::Path;

use semver::Version;
use xtask::check::{CheckError, CheckKind, CheckReport};
use xtask::release::{ReleaseError, ReleaseOutcome, release};
use xtask::targets::{TargetError, TargetOutcome, TargetsArgs};

#[test]
fn release_entrypoint_is_public_and_typed() {
    let entrypoint: fn(&Path, &Version, bool) -> Result<ReleaseOutcome, ReleaseError> = release;
    let _ = entrypoint;
}

#[test]
fn check_and_target_entrypoints_are_public_and_typed() {
    let check: fn(&Path, CheckKind) -> Result<CheckReport, CheckError> = xtask::check::run;
    let targets: fn(&TargetsArgs) -> Result<TargetOutcome, TargetError> = xtask::targets::run;
    let _ = (check, targets);
}
