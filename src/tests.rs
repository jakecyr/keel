use super::*;
#[path = "feature_tests.rs"]
mod feature_tests;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::{process::Stdio,thread};

fn options() -> TestOptions {
    TestOptions {
        cases: 200,
        seed: 42,
        timeout_ms: 2000,
        filter: None,
        replay: None,
        shrink: true,
        memory_mib: 256,
        budget_ms: 30_000,
    }
}
fn evaluate(source: &str) -> Value {
    let (p, a) = checked(source).unwrap();
    run_tests(source, &p, &a, &options()).unwrap()
}
fn rejected(source: &str, kind: &str) {
    let error = checked(source).err().expect("invalid program was accepted");
    assert_eq!(error["diagnostics"][0]["kind"], kind, "{error}");
}

#[test]
fn static_type_errors() {
    rejected("fn f() -> Int { return true }", "type_mismatch");
    rejected("fn f(value: Text) {}", "parameter_mode");
    rejected("fn f() { let x = true + false }", "operator_type");
    rejected("fn f() -> Int { if true { return 1 } }", "missing_return");
    rejected("fn f() { let x = 1 x = 2 }", "immutable");
    rejected("fn f() { missing() }", "unknown_function");
    rejected("fn f() { let x = 1 let x = 2 }", "duplicate_name");
    rejected("fn f() { let x = hole(\"untyped\") }", "hole_type");
    rejected("fn main(value: Int) {}", "entrypoint");
    rejected("test \"early\" { return; assert false }", "test_return");
    rejected("fn f() { let x = 9223372036854775808 }", "integer_literal");
    rejected("fn f() { let x = 18446744073709551616 }", "integer_literal");
}
#[test]
fn ownership_rejects_aliases_and_moves() {
    rejected(
        "fn f() { let x = text.clone(\"a\") let y = x }",
        "ownership",
    );
    rejected(
        "fn f() { let x = text.clone(\"a\") let y = take x assert x == y }",
        "use_after_move",
    );
    rejected("fn f(x: read Text) -> Text { return x }", "ownership");
    rejected(
        "fn f(x: read Text) -> Text { return take x }",
        "invalid_move",
    );
    rejected(
        "fn f() { let x = text.clone(\"a\") while true { let y = take x } }",
        "loop_move",
    );
    rejected(
        "fn f(flag: Bool) { let x = text.clone(\"a\") if flag { let y = take x } assert x == \"a\" }",
        "use_after_move",
    );
    rejected(
        "fn eat(a: read Text, b: take Text) {} fn f() { let x = text.clone(\"a\") eat(x, take x) }",
        "borrow_conflict",
    );
    rejected(
        "fn eat(a: take Text) -> Text { return text.clone(\"a\") } fn f() { let x = text.clone(\"a\") assert x == eat(take x) }",
        "borrow_conflict",
    );
}
#[test]
fn effects_and_contracts_are_checked() {
    rejected("fn f() { io.println(\"hello\") }", "effect_not_allowed");
    rejected(
        "fn loud() effects { io.stdout } { io.println(\"x\") } fn f() { loud() }",
        "effect_not_allowed",
    );
    rejected("fn f() effects { net.anything } {}", "effect");
    rejected(
        "fn f() -> Int ensures hole(\"oracle\") { return 1 }",
        "contract_hole",
    );
    rejected(
        "fn log() -> Bool effects { io.stdout } { return true } fn f() -> Int ensures log() { return 1 }",
        "effect_not_allowed",
    );
    rejected(
        "fn handler(x: read Text) -> Text effects { io.stdout } { return text.clone(x) } fn main() effects { net.listen } { http.serve(8080, handler) }",
        "handler",
    );
}
#[test]
fn holes_have_context_and_do_not_pass() {
    let source = include_str!("../examples/holes.keel");
    let (p, a) = checked(source).unwrap();
    assert_eq!(a.holes[0].expected, syntax::Type::Int);
    assert_eq!(a.holes[0].bindings["value"], syntax::Type::Int);
    let report = run_tests(source, &p, &a, &options()).unwrap();
    assert_eq!(report["status"], "BLOCKED");
    assert_eq!(report["tests"][0]["status"], "TESTED");
    assert_eq!(report["tests"][1]["status"], "BLOCKED");
}
#[test]
fn web_server_examples_and_properties_pass() {
    assert_eq!(
        evaluate(include_str!("../examples/web_server.keel"))["status"],
        "TESTED"
    );
}
#[test]
fn ownership_examples_pass() {
    assert_eq!(
        evaluate(include_str!("../examples/ownership.keel"))["status"],
        "TESTED"
    );
}
#[test]
fn arithmetic_traps_in_native_code() {
    let report = evaluate(
        r#"
        test "add overflow" { let x = 9223372036854775807 + 1 }
        test "subtract overflow" { let x = -9223372036854775808 - 1 }
        test "multiply overflow" { let x = 9223372036854775807 * 2 }
        test "divide overflow" { let x = -9223372036854775808 / -1 }
        test "negate overflow" { let x = -(-9223372036854775808) }
        test "zero division" { let x = 1 / 0 }
        test "zero remainder" { let x = 1 % 0 }
        test "remainder overflow" { let x = -9223372036854775808 % -1 }
    "#,
    );
    let tests = report["tests"].as_array().unwrap();
    assert_eq!(tests.len(), 8);
    for (i, test) in tests.iter().enumerate() {
        assert_eq!(test["status"], "FAILED");
        assert_eq!(
            test["failure"]["kind"],
            if i == 5 || i == 6 {
                "division_by_zero"
            } else {
                "overflow"
            }
        );
    }
}
#[test]
fn native_matches_rust_integer_reference() {
    let mut source = String::from("test \"integer reference\" {\n");
    let mut seed = 7_u64;
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = (seed % 2_000_001) as i64 - 1_000_000;
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = (seed % 2_000_001) as i64 - 1_000_000;
        let b = if b == 0 { 1 } else { b };
        for (op, expected) in [
            ("+", a + b),
            ("-", a - b),
            ("*", a * b),
            ("/", a / b),
            ("%", a % b),
        ] {
            source.push_str(&format!("assert ({a}) {op} ({b}) == ({expected})\n"));
        }
    }
    source.push('}');
    assert_eq!(evaluate(&source)["status"], "TESTED");
}
#[test]
fn left_to_right_short_circuit_and_cleanup() {
    let report = evaluate(
        r#"
        fn first() -> Int { assert false return 1 }
        fn second() -> Int { return 1 / 0 }
        fn add(a: Int, b: Int) -> Int { return a + b }
        fn choose(x: take Text, flag: Bool) -> Text {
            if flag { return x }
            return text.concat(x, "!")
        }
        test "left argument fails first" { let x = add(first(), second()) }
        test "short circuit" { assert true || first() == 0 assert !(false && second() == 0) }
        test "conditional ownership" {
            let x = text.clone("a")
            let y = choose(take x, true)
            assert y == "a"
            let z = text.clone("b")
            assert choose(take z, false) == "b!"
        }
        test "restore moved value per loop" {
            var x = text.clone("a") var i = 0
            while i < 100 { let previous = take x x = text.concat(previous, "b") i = i + 1 }
            assert text.len(x) == 101
        }
    "#,
    );
    assert_eq!(report["tests"][0]["failure"]["kind"], "assertion_failure");
    for i in 1..4 {
        assert_eq!(report["tests"][i]["status"], "TESTED", "{report}");
    }
}
#[test]
fn contracts_are_enforced() {
    let report = evaluate(
        r#"
        fn require_positive(n: Int) -> Int requires n > 0 { return n }
        fn ensure_positive(n: Int) -> Int ensures result > 0 { return n }
        test "requires" { let n = require_positive(0) }
        test "ensures" { let n = ensure_positive(0) }
    "#,
    );
    assert_eq!(
        report["tests"][0]["failure"]["kind"],
        "precondition_failure"
    );
    assert_eq!(
        report["tests"][1]["failure"]["kind"],
        "postcondition_failure"
    );
}
#[test]
fn counterexamples_shrink_and_replay() {
    let source = r#"property "under ten" (n in gen.int(min: 0, max: 1000)) { assert n < 10 }"#;
    let (p, a) = checked(source).unwrap();
    let report = run_tests(source, &p, &a, &options()).unwrap();
    let failure = &report["tests"][0]["failure"];
    assert_eq!(failure["value"], 10);
    assert_eq!(failure["shrunk"], true);
    let mut opts = options();
    opts.replay = Some(10);
    let replay = run_tests(source, &p, &a, &opts).unwrap();
    assert_eq!(replay["status"], "FAILED");
    assert_eq!(replay["tests"][0]["failure"]["value"], 10);
}
#[test]
fn timeout_and_no_tests_are_unknown() {
    let source = "test \"loop\" { while true {} }";
    let (p, a) = checked(source).unwrap();
    let mut opts = options();
    opts.timeout_ms = 50;
    assert_eq!(
        run_tests(source, &p, &a, &opts).unwrap()["status"],
        "UNKNOWN"
    );
    assert_eq!(evaluate("fn main() {}")["status"], "UNKNOWN");
}
#[test]
fn integer_generators_include_bounds_and_handle_entire_int_range() {
    let report = evaluate(
        r#"
        property "minimum" (n in gen.int(min: -9223372036854775808, max: 9223372036854775807)) { assert n >= -9223372036854775808 }
        property "singleton" (n in gen.int(min: 7, max: 7)) { assert n == 7 }
    "#,
    );
    assert_eq!(report["status"], "TESTED");
}
#[test]
fn structural_edits_are_validated_and_preserve_contracts() {
    let source = "// keep this\npub fn answer() -> Int ensures result == 42 { return 0 }\ntest \"answer\" { assert answer() == 42 }\n";
    let (p, _) = checked(source).unwrap();
    let temp = Temp::new().unwrap();
    let path = temp.0.join("app.keel");
    let request = temp.0.join("edit.json");
    fs::write(&path, source).unwrap();
    let args = vec!["--request".into(), request.to_string_lossy().to_string()];
    let mut edit_request = json!({"base_revision":revision(source),"target":"fn:answer","operation":"replace_body","source":"{ return 42 }","run":"affected_checks_and_tests"});
    fs::write(&request, edit_request.to_string()).unwrap();
    let result = edit(&path, source, &p, &args).unwrap();
    assert_eq!(result["status"], "APPLIED");
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, source.replace("{ return 0 }", "{ return 42 }"));
    let (p2, _) = checked(&after).unwrap();
    assert_eq!(edit(&path, &after, &p2, &args).unwrap()["status"], "FAILED");
    assert_eq!(fs::read_to_string(&path).unwrap(), after);
    edit_request["base_revision"] = json!(revision(&after));
    edit_request["source"] = json!("{ return true }");
    fs::write(&request, edit_request.to_string()).unwrap();
    assert_eq!(edit(&path, &after, &p2, &args).unwrap()["applied"], false);
    edit_request["source"] = json!("{ return 41 }");
    fs::write(&request, edit_request.to_string()).unwrap();
    assert_eq!(edit(&path, &after, &p2, &args).unwrap()["applied"], false);
    edit_request["source"] = json!("{ return 42 } fn injected() {}");
    fs::write(&request, edit_request.to_string()).unwrap();
    assert!(edit(&path, &after, &p2, &args).is_err());
    edit_request["source"] = json!("effects { io.stdout } { return 42 }");
    fs::write(&request, edit_request.to_string()).unwrap();
    assert!(edit(&path, &after, &p2, &args).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), after);
}
#[test]
fn inspect_reports_dependencies_callers_and_truncation() {
    let source = include_str!("../examples/web_server.keel");
    let (p, a) = checked(source).unwrap();
    let result = inspect(
        source,
        &p,
        &a,
        &[
            "--symbol".into(),
            "route".into(),
            "--max-chars".into(),
            "10".into(),
        ],
    )
    .unwrap();
    assert_eq!(result["incomplete"], true);
    assert_eq!(
        result["functions"][0]["source"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        10
    );
    assert!(
        result["functions"][0]["callers"]
            .as_array()
            .unwrap()
            .contains(&json!("main"))
    );
}

#[test]
fn native_text_lifetimes_under_address_and_undefined_behavior_sanitizers() {
    let source = r#"
        fn forward(x: take Text) -> Text { return x }
        fn join(a: read Text, b: take Text) -> Text { return text.concat(a,b) }
        test "text lifetime stress" {
            var text = text.clone("start") var i = 0
            while i < 1000 {
                let number = text.from_int(i)
                let original = take text
                text = join(original, forward(take number))
                if i % 2 == 0 { text = text.clone("reset") }
                assert text.len(text) > 0
                i = i + 1
            }
            let response = http.response(200, text)
            assert http.body(response) == text
            assert text.clone("héllo 🌊") == "héllo 🌊"
        }
    "#;
    let (p, a) = checked(source).unwrap();
    let temp = Temp::new().unwrap();
    let cpath = temp.0.join("sanitized.c");
    let binary = temp.0.join("sanitized");
    fs::write(&cpath, native::emit(&p, &a, true)).unwrap();
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
        "sanitizer compiler failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut limits=options(); limits.memory_mib=0;
    let (state, failure) = run_worker(&binary, 0, &limits, None).unwrap();
    assert_eq!(state, "TESTED", "{failure:?}");
}

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn request(port: u16, text: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(text.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
#[test]
fn real_http_server_requires_authority_and_serves_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let source = include_str!("../examples/web_server.keel").replace("8080", &port.to_string());
    let (p, a) = checked(&source).unwrap();
    let temp = Temp::new().unwrap();
    let binary = temp.0.join("server");
    compile(&p, &a, false, &binary, None).unwrap();
    let denied = Command::new(&binary).output().unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("permission_denied_net"));
    let wrong = Command::new(&binary)
        .arg("--allow-net=0.0.0.0:8080")
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    let mut server = Server(
        Command::new(&binary)
            .arg(format!("--allow-net=127.0.0.1:{port}"))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            drop(stream);
            break;
        }
        assert!(Instant::now() < deadline, "server did not start");
        assert!(server.0.try_wait().unwrap().is_none(), "server exited");
        thread::sleep(Duration::from_millis(10));
    }
    for (path, status, body) in [
        ("/", "200", "Hello from Keel!\n"),
        ("/health?probe=1", "200", "ok\n"),
        ("/square", "200", "144\n"),
        ("/unknown", "404", "not found\n"),
    ] {
        let response = request(
            port,
            &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        );
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{response}"
        );
        assert_eq!(response.split_once("\r\n\r\n").unwrap().1, body);
        assert!(response.contains(&format!("Content-Length: {}\r\n", body.len())));
    }
    assert!(
        request(port, "POST / HTTP/1.1\r\nHost: localhost\r\n\r\n").starts_with("HTTP/1.1 405")
    );
    assert!(request(port, "nonsense\r\n\r\n").starts_with("HTTP/1.1 400"));
    assert!(request(port, "GET / HTTP/1.1\r\n").starts_with("HTTP/1.1 400"));
}
