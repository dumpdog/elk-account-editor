use elk_panel::{json_syntax_check, parse_model_body};

#[test]
fn syntax_errors_use_the_dotnet_wording() {
    let e = json_syntax_check(b"{not json").unwrap_err();
    assert_eq!(e, "'n' is an invalid start of a property name. Expected a '\"'. Path: $ | LineNumber: 0 | BytePositionInLine: 1.");
    assert!(json_syntax_check(b"{}").is_ok());
}

#[test]
fn model_body_null_empty_and_object() {
    assert_eq!(parse_model_body(b"null"), Ok(None));
    assert!(parse_model_body(b"{").is_err());
    assert!(parse_model_body(b"{}").unwrap().is_some());
    assert!(parse_model_body(b"[]").is_err());
}
