use super::*;

#[test]
fn bounded_regex_replacement_matches_regex_capture_syntax() {
    let regex = Regex::new(r"(?P<word>[a-z]+)-(\d+)").unwrap();
    let replacement = "${word}:$2:$$:$missing:$1x";
    let expected = regex.replace_all("ab-12", replacement).into_owned();
    assert_eq!(
        regex_replace_all(&regex, "ab-12", replacement, expected.len(), "fixture").unwrap(),
        expected
    );
    assert!(
        regex_replace_all(&regex, "ab-12", replacement, expected.len() - 1, "fixture").is_err()
    );
}

#[test]
fn bounded_regex_replacement_checks_before_large_appends() {
    let regex = Regex::new("(.*)").unwrap();
    let input = "x".repeat(1024 * 1024);
    let error = regex_replace_all(&regex, &input, "$1$1", 16, "fixture").unwrap_err();
    assert!(error.to_string().contains("16-byte"));

    let regex = Regex::new("x").unwrap();
    let replacement = "y".repeat(1024 * 1024);
    let error = regex_replace_all(&regex, "x", &replacement, 16, "fixture").unwrap_err();
    assert!(error.to_string().contains("16-byte"));
}

#[test]
fn replacement_template_matches_regex_edge_syntax() {
    let regex = Regex::new(r"(?P<word>[a-z]+)-(\d+)").unwrap();
    for replacement in [
        "$",
        "$$",
        "${word}",
        "${word",
        "$0/$1/$2/$99",
        "$1_$2",
        "${1}_${2}",
        "${missing}:$missing",
        "pre-$word-post",
    ] {
        let expected = regex.replace_all("ab-12 cd-34", replacement).into_owned();
        let actual = regex_replace_all(
            &regex,
            "ab-12 cd-34",
            replacement,
            expected.len(),
            "fixture",
        )
        .unwrap();
        assert_eq!(actual, expected, "replacement={replacement}");
    }
}
