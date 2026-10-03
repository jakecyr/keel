use super::*;

#[test]
fn collections_example_contracts_and_properties() {
    assert_eq!(
        evaluate(include_str!("../examples/collections.keel"))["status"],
        "TESTED"
    );
}

#[test]
fn collection_ownership_and_exclusive_borrows() {
    for (source, kind) in [
        ("fn f() { let a = [1] let b = a }", "ownership"),
        (
            "fn f() { let a = [1] let b = take a assert list.len(a) == 0 }",
            "use_after_move",
        ),
        (
            "fn f(a: read List<Int>) -> List<Int> { return take a }",
            "invalid_move",
        ),
        ("fn f() { let a = [1] list.push(edit a, 2) }", "immutable"),
        ("fn f() { var a = [1] list.push(a, 2) }", "invalid_edit"),
        (
            "fn f(a: edit List<Int>, b: read List<Int>) {} fn g() { var a = [1] f(edit a, a) }",
            "borrow_conflict",
        ),
        (
            "fn f(a: read List<Int>, b: edit List<Int>) {} fn g() { var a = [1] f(a, edit a) }",
            "borrow_conflict",
        ),
        (
            "fn f(a: edit List<Int>, b: edit List<Int>) {} fn g() { var a = [1] f(edit a, edit a) }",
            "borrow_conflict",
        ),
        (
            "fn f() { var a = [1] list.push(edit a, list.len(a)) }",
            "borrow_conflict",
        ),
        (
            "fn f(a: read List<Int>) { list.push(edit a, 2) }",
            "immutable",
        ),
        (
            "fn f() { var a = [1] for n in a { list.push(edit a, n) } }",
            "borrow_conflict",
        ),
        (
            "fn f() { var a = [1] for n in a { a = [] } }",
            "borrow_conflict",
        ),
        (
            "fn f() { var a = [1] for n in a { let b = take a } }",
            "borrow_conflict",
        ),
        (
            "fn f() { let a = [1] while true { let b = take a } }",
            "loop_move",
        ),
        ("fn f() { let a = result.ok(1) let b = a }", "ownership"),
        (
            "fn f() -> Text { match result.err(\"error\") { Ok(v) => { return \"ok\" } Err(e) => { return e } } }",
            "ownership",
        ),
        (
            "fn f() { var a = result.ok(1) match a { Ok(v) => { a = result.ok(2) } Err(e) => {} } }",
            "borrow_conflict",
        ),
    ] {
        rejected(source, kind);
    }
}

#[test]
fn matches_are_exhaustive_and_typed() {
    for (source, kind) in [
        (
            "fn f() { match option.none() { Some(v) => {} } }",
            "non_exhaustive_match",
        ),
        (
            "fn f() { match option.none() { None => {} None => {} } }",
            "match_variant",
        ),
        (
            "fn f() { match option.none() { None(v) => {} Some(x) => {} } }",
            "match_variant",
        ),
        (
            "fn f() { match option.none() { None => {} Some => {} } }",
            "match_variant",
        ),
        (
            "fn f() { match option.none() { Ok(v) => {} Err(e) => {} } }",
            "match_variant",
        ),
        (
            "fn f() { match 1 { Some(v) => {} None => {} } }",
            "match_type",
        ),
        ("fn f() { let a = [true] }", "type_mismatch"),
        ("fn f() { for v in 1 {} }", "type_mismatch"),
    ] {
        rejected(source, kind);
    }
    assert_eq!(
        evaluate(
            r#"
        fn value(x: Option<Int>) -> Int {
            match x { Some(n) => { return n } None => { return -1 } }
        }
        test "both variants return" { assert value(option.some(42)) == 42 assert value(option.none()) == -1 }
    "#
        )["status"],
        "TESTED"
    );
}

#[test]
fn integer_parsing_is_recoverable_strict_and_full_range() {
    assert_eq!(
        evaluate(
            r#"
        fn valid(raw: read Text, expected: Int) {
            match text.parse_int(raw) { Ok(value) => { assert value == expected } Err(error) => { assert false } }
        }
        fn invalid(raw: read Text) {
            match text.parse_int(raw) { Ok(value) => { assert false } Err(error) => { assert text.len(error) > 0 } }
        }
        test "valid" { valid("0", 0) valid("-0", 0) valid("001", 1) valid("9223372036854775807", 9223372036854775807) valid("-9223372036854775808", -9223372036854775808) }
        test "invalid" { invalid("") invalid("-") invalid("+1") invalid(" 1") invalid("1 ") invalid("1.0") invalid("1e2") invalid("é") invalid("9223372036854775808") invalid("-9223372036854775809") invalid("9999999999999999999999999999") }
        property "roundtrip" (value in gen.int(min: -9223372036854775808, max: 9223372036854775807)) { valid(text.from_int(value), value) }
    "#
        )["status"],
        "TESTED"
    );
}

#[test]
fn list_bounds_trap_and_safe_get_returns_none() {
    let report = evaluate(
        r#"
        test "negative" { let a = [1] let b = list.at(a, -1) }
        test "past end" { let a = [1] let b = list.at(a, 1) }
        test "empty" { let b = list.at([], 0) }
        test "set" { var a = [1] list.set(edit a, 1, 2) }
        test "safe" { assert list.get([], 0) == option.none() assert list.get([1], 0) == option.some(1) }
    "#,
    );
    for case in &report["tests"].as_array().unwrap()[..4] {
        assert_eq!(case["failure"]["kind"], "bounds");
        assert!(case["failure"]["offset"].as_u64().unwrap() > 0);
    }
    assert_eq!(report["tests"][4]["status"], "TESTED");
}

#[test]
fn syntax_resource_limits_are_structured_errors() {
    rejected(
        &format!(
            "fn f() {{ let x = {}1{} }}",
            "(".repeat(100),
            ")".repeat(100)
        ),
        "resource_limit",
    );
    rejected(
        &format!("fn f() {{ let x = {}true }}", "!".repeat(100)),
        "resource_limit",
    );
    rejected(
        &format!("fn f() {{ let x = 1{} }}", "+1".repeat(100)),
        "resource_limit",
    );
    rejected(&" ".repeat(4 * 1024 * 1024 + 1), "resource_limit");
}

#[test]
fn native_collection_growth_cleanup_and_early_returns() {
    assert_eq!(
        evaluate(
            r#"
        fn grow(items: edit List<Int>) { var i = 0 while i < 1000 { list.push(edit items, i) i = i + 1 } }
        fn replacement(items: edit List<Int>) { items = [10, 20] }
        fn consume(items: take List<Int>) -> List<Int> { return items }
        fn error_message() -> Text {
            match result.err(text.clone("message")) { Ok(n) => { return "unexpected" } Err(e) => { return text.clone(e) } }
        }
        test "growth" {
            var items = [] grow(edit items) assert list.len(items) == 1000
            for value in items { assert list.at(items, value) == value }
            replacement(edit items) assert items == [10, 20]
            let moved = consume(take items) assert moved == [10, 20]
            assert error_message() == "message"
            var i = 0 while i < 100 { var temp = [1, 2] temp = list.clone(temp) let e = result.err(text.from_int(i)) i = i + 1 }
        }
    "#
        )["status"],
        "TESTED"
    );
}

#[test]
fn deduplication_matches_independent_reference_over_generated_lists() {
    let mut source = include_str!("../examples/collections.keel").to_string();
    source.push_str("\ntest \"independent generated list oracle\" {\n");
    let mut seed = 1357_u64;
    for length in 0..70 {
        let mut input = Vec::new();
        let mut expected = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..length {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = (seed % 21) as i64 - 10;
            input.push(n);
            if seen.insert(n) {
                expected.push(n);
            }
        }
        source.push_str(&format!(
            "assert stable_unique({input:?}) == {expected:?}\n"
        ));
    }
    source.push('}');
    assert_eq!(evaluate(&source)["status"], "TESTED");
}

#[test]
fn collections_and_results_under_native_memory_sanitizers() {
    let mut source = include_str!("../examples/collections.keel").to_string();
    source.push_str(r#"
        fn pass(items: take List<Int>) -> List<Int> { return items }
        fn change(items: edit List<Int>) { items = [1, 2, 3] }
        fn message(input: take Result<Int, Text>) -> Text {
            match input { Ok(n) => { return text.from_int(n) } Err(error) => { return text.clone(error) } }
        }
        test "memory lifetime stress" {
            var index = 0
            while index < 500 {
                var items = [index]
                var j = 0 while j < 100 { list.push(edit items, j) j = j + 1 }
                let original = pass(take items)
                items = list.clone(original)
                change(edit items)
                for n in list.clone(items) { assert n > 0 }
                var failure = result.err(text.from_int(index))
                failure = result.err(text.clone("replacement"))
                assert message(take failure) == "replacement"
                assert message(result.ok(index)) == text.from_int(index)
                index = index + 1
            }
        }
    "#);
    let (program, analysis) = checked(&source).unwrap();
    let temp = Temp::new().unwrap();
    let cpath = temp.0.join("collections-sanitized.c");
    let binary = temp.0.join("collections-sanitized");
    fs::write(&cpath, native::emit(&program, &analysis, true)).unwrap();
    let output = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()))
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-fsanitize=address,undefined",
            "-fno-omit-frame-pointer",
        ])
        .arg(&cpath)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut limits = options();
    limits.memory_mib = 0;
    // Sanitizer initialization on hosted Intel macOS is much slower than an
    // ordinary native test. Keep the same assertions and a bounded deadline.
    limits.timeout_ms = 10_000;
    for index in 0..program.tests.len() {
        let (state, failure) = run_worker(&binary, index, &limits, None).unwrap();
        assert_eq!(
            state, "TESTED",
            "{}: {failure:?}",
            program.tests[index].name
        );
    }
}
