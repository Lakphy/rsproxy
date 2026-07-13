use clap::Parser;

use super::Cli;

#[test]
fn release_rejects_non_semver_input_during_argument_parsing() {
    let error = Cli::try_parse_from(["xtask", "release", "v0.2.0", "--check"])
        .expect_err("a leading v is not a semantic version");
    assert_eq!(error.exit_code(), 2);
}
