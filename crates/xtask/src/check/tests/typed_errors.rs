use super::super::typed_errors::violations_for_test;
use super::fixture::Fixture;

#[test]
fn typed_error_scan_accepts_domain_and_borrowed_errors() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write(
        "crates/example/src/lib.rs",
        "pub fn typed() -> Result<(), std::io::Error> { Ok(()) }\n\
         pub fn borrowed<'a>() -> Result<(), &'a str> { Ok(()) }\n",
    );
    assert!(
        violations_for_test(fixture.root())
            .expect("scan typed errors")
            .is_empty()
    );
}

#[test]
fn typed_error_scan_rejects_owned_and_static_string_channels() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write(
        "crates/example/src/lib.rs",
        "type A = Result<(), String>;\n\
         type B = std::result::Result<(), &'static str>;\n\
         type C = crate::Result<(), std::string::String>;\n",
    );
    let violations = violations_for_test(fixture.root()).expect("scan string errors");
    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("2 `Result<_, String>`"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("static str"))
    );
}
