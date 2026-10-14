use crate::{check, run};

#[test]
fn error_identities_are_specific_and_independent_of_declaration_names() {
    for name in ["first", "renamed"] {
        let error = check(&format!("print {name};")).unwrap_err();
        assert_eq!(error.diagnostics()[0].code(), "ORNI-E103");
    }
    for (source, code) in [
        ("let value: int8 = 128;", "ORNI-E104"),
        ("let value: string<4> = \"Lucas\";", "ORNI-E106"),
        ("let value: bool = 1;", "ORNI-E107"),
        (
            "contract Clock { func now() -> int; } func start(clock: Clock) -> unit {} start();",
            "ORNI-E102",
        ),
    ] {
        assert_eq!(check(source).unwrap_err().diagnostics()[0].code(), code);
    }
    assert_eq!(
        run("print 1 / 0;").unwrap_err().diagnostics()[0].code(),
        "ORNI-E131"
    );
}

#[test]
fn rejects_contract_defaults_and_duplicate_parameters() {
    assert!(
        check("contract Logger { func log(message: string = \"default\") -> unit; }")
            .unwrap_err()
            .to_string()
            .contains("cannot declare default values")
    );
    assert!(
        check("func add(value: int, value: int) -> int { return value; }")
            .unwrap_err()
            .to_string()
            .contains("already declared")
    );
}

#[test]
fn ignores_bindings_for_unselected_provider_implementations() {
    check("contract Clock { func now() -> int; } contract Audit { func record() -> unit; } provider TimedAudit(clock: Clock) for Audit { func record() -> unit {} }").unwrap();
}

#[test]
fn validates_signed_literals_unsigned_negation_and_narrowing() {
    assert_eq!(
        run("let minimum = -128; print minimum; print 18446744073709551615;").unwrap(),
        "-128\n18446744073709551615\n"
    );
    assert!(check("let port: uint16 = 80; print -port;").is_err());
    assert!(check("let value: int8 = 128;").is_err());
    assert!(check("func get() -> uint8 { return 256; }").is_err());
    assert!(check("func take(value: uint8) -> unit {} take(256);").is_err());
    assert!(check("let value = 18446744073709551616;").is_err());
    assert!(check("let value = -9223372036854775809;").is_err());
}

#[test]
fn string_escapes_match_the_editor_and_reject_unknown_sequences() {
    assert_eq!(run(r#"print "a\nb\t\"\\";"#).unwrap(), "a\nb\t\"\\\n");
    assert!(check(r#"print "a\qb";"#).is_err());
}

#[test]
fn nested_module_check_uses_manifest_root_and_does_not_require_bindings() {
    crate::check_module("examples/experimental/framework_project/providers/console_audit.orni")
        .unwrap();
}

#[test]
fn retained_resource_counters_release_block_and_function_scopes() {
    let limits = crate::ExecutionLimits {
        max_retained_string_bytes: 3,
        max_variables: 1,
        ..Default::default()
    };
    assert_eq!(
        crate::run_with_limits(
            "{ let first = \"abc\"; } { let second = \"abc\"; print second; }",
            limits
        )
        .unwrap(),
        "abc\n"
    );
    let limits = crate::ExecutionLimits {
        max_retained_string_bytes: 3,
        ..Default::default()
    };
    assert_eq!(
        crate::run_with_limits(
            "func say(value: string) -> unit { print value; } say(\"abc\"); say(\"abc\");",
            limits
        )
        .unwrap(),
        "abc\nabc\n"
    );
}
