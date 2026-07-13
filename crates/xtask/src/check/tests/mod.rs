mod api;
mod fixture;
mod layout;
mod lines;
mod typed_errors;
mod whistle;
mod workflows;

use super::CheckKind;
use super::expanded_checks;

#[test]
fn all_expands_every_check_in_the_required_order() {
    assert_eq!(
        expanded_checks(CheckKind::All),
        &[
            CheckKind::Api,
            CheckKind::Lines,
            CheckKind::Layout,
            CheckKind::TypedErrors,
            CheckKind::Workflows,
        ],
    );
}
