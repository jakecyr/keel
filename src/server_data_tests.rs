use super::*;

const CATALOG: &str = concat!(
    include_str!("../examples/catalog_api/main.keel"),
    "\n",
    include_str!("../examples/catalog_api/routes.keel"),
    "\n",
    include_str!("../examples/catalog_api/tests.keel")
);

fn text_case(source: &mut String, call: &str, expected: std::result::Result<&str, &str>) {
    let (ok, expected) = match expected {
        Ok(s) => (true, s),
        Err(s) => (false, s),
    };
    let literal = serde_json::to_string(expected).unwrap();
    let assertion = if ok {
        format!("Ok(value)=>{{assert value=={literal}}} Err(_error)=>{{assert false}}")
    } else {
        format!("Ok(_value)=>{{assert false}} Err(error)=>{{assert error=={literal}}}")
    };
    source.push_str(&format!("match {call} {{ {assertion} }}\n"));
}
fn literal(s: &str) -> String {
    serde_json::to_string(s).unwrap().replace("\\u0000", "\0")
}

fn corpus() -> String {
    let mut source = format!("{CATALOG}\ntest \"request metadata and JSON replacement\" {{\n");
    for (target, key, expected) in [
        (
            "/search?q=green+tea%20%F0%9F%8D%B5",
            "q",
            Ok("green tea 🍵"),
        ),
        ("/s?q=%26%3D%2B%25", "q", Ok("&=+%")),
        ("/s?%71=one", "q", Ok("one")),
        ("/s?q&x=1", "q", Ok("")),
        ("/s?q=one?two", "q", Ok("one?two")),
        ("/s?Q=a&q=b", "q", Ok("b")),
        ("/s?q=a&%71=b", "q", Err("duplicate HTTP query parameter")),
        ("/s?x=a", "q", Err("HTTP query parameter not found")),
        ("/s", "q", Err("HTTP query parameter not found")),
        ("/s?q=a&x=%FF", "q", Err("invalid HTTP query")),
        ("/s?q=%", "q", Err("invalid HTTP query")),
        ("/s?q=%0", "q", Err("invalid HTTP query")),
        ("/s?q=%GG", "q", Err("invalid HTTP query")),
        ("/s?q=%00", "q", Err("invalid HTTP query")),
        ("/s?q=%0D%0A", "q", Err("invalid HTTP query")),
        ("/s?q=%C0%AF", "q", Err("invalid HTTP query")),
        ("/s?q=%ED%A0%80", "q", Err("invalid HTTP query")),
        ("/s?q=a#fragment", "q", Err("invalid HTTP query")),
        ("https://host/?q=a", "q", Err("invalid HTTP query")),
    ] {
        text_case(
            &mut source,
            &format!("http.query({}, {})", literal(target), literal(key)),
            expected,
        );
    }
    for (headers, name, expected) in [
        (
            "Content-Type: \tapplication/json \t\r\nX-Name: tea 🍵\r\n",
            "CONTENT-type",
            Ok("application/json"),
        ),
        ("X: \r\n", "x", Ok("")),
        ("X: one:two\r\n", "x", Ok("one:two")),
        ("X: a\r\nx: b\r\n", "X", Err("duplicate HTTP header")),
        ("X: a\r\n", "y", Err("HTTP header not found")),
        ("", "x", Err("HTTP header not found")),
        (
            "X: a\r\nBad Header: b\r\n",
            "x",
            Err("invalid HTTP headers"),
        ),
        ("X: a\r\n folded\r\n", "x", Err("invalid HTTP headers")),
        ("X: a\n", "x", Err("invalid HTTP headers")),
        ("X: a\r\n\r\n", "x", Err("invalid HTTP headers")),
        ("X: a\0b\r\n", "x", Err("invalid HTTP headers")),
        ("X: a\r\n", "", Err("invalid HTTP headers")),
    ] {
        text_case(
            &mut source,
            &format!("http.header({}, {})", literal(headers), literal(name)),
            expected,
        );
    }
    for (document, pointer, replacement, expected) in [
        (
            " {\"a\": [1, 2], \"score\":0.9900} ",
            "/a/1",
            "42",
            Ok(" {\"a\": [1, 42], \"score\":0.9900} "),
        ),
        (
            "{\"a/b\":{\"~x\":null}}",
            "/a~1b/~0x",
            "[true, false]",
            Ok("{\"a/b\":{\"~x\":[true, false]}}"),
        ),
        (" 0 ", "", "{\"n\":1e999}", Ok(" {\"n\":1e999} ")),
        ("{\"a\":1,\"a\":2}", "/a", "3", Ok("{\"a\":1,\"a\":3}")),
        (
            "{\"a\":1,\"\\u0061\":2}",
            "/a",
            "3",
            Ok("{\"a\":1,\"\\u0061\":3}"),
        ),
        ("[1]", "/01", "0", Err("JSON path not found")),
        ("[1]", "/-", "0", Err("JSON path not found")),
        ("[1]", "/1", "0", Err("JSON path not found")),
        ("{}", "/missing", "0", Err("JSON path not found")),
        ("{}", "/~2", "0", Err("invalid JSON pointer")),
        ("{}", "x", "0", Err("invalid JSON pointer")),
        ("[1]", "/0", "1,2", Err("invalid JSON")),
        ("[1]", "/0", "\"\\ud800\"", Err("invalid JSON")),
        ("{\"bad\":1,}", "", "{}", Err("invalid JSON")),
    ] {
        text_case(
            &mut source,
            &format!(
                "json.set({}, {}, {})",
                literal(document),
                literal(pointer),
                literal(replacement)
            ),
            expected,
        );
    }
    // Inputs individually fit depth 64; replacing a nested node must enforce the output bound too.
    let nested = format!("{}0{}", "[".repeat(64), "]".repeat(64));
    text_case(
        &mut source,
        &format!("json.set(\"[0]\", \"/0\", {})", literal(&nested)),
        Err("invalid JSON"),
    );
    for (document, count) in [("[]", 0), ("[null,true,{},[1,2],\"a,b\"]", 5)] {
        source.push_str(&format!("match json.array_len({}) {{ Ok(n)=>{{assert n=={count}}} Err(_e)=>{{assert false}} }}\n", literal(document)));
    }
    for document in ["{}", "null", "[1,]", "[\"\\ud800\"]"] {
        source.push_str(&format!("match json.array_len({}) {{ Ok(_n)=>{{assert false}} Err(e)=>{{assert text.len(e)>0}} }}\n", literal(document)));
    }
    source.push_str("assert http.path(\"/a%20b?q=x\")==\"/a%20b\"\n}");
    source
}

#[test]
fn server_data_independent_native_reference_and_sanitizers() {
    let source = corpus();
    let (p, a) = checked(&source).unwrap();
    let report = test_engine(&source, &p, &a, &["--engine".into(), "both".into()]).unwrap();
    assert_eq!(report["status"], "TESTED", "{report}");
    assert_eq!(report["differential"]["mismatches"], json!([]), "{report}");
    let temp = Temp::new().unwrap();
    let cpath = temp.0.join("corpus.c");
    let binary = temp.0.join("corpus");
    fs::write(&cpath, native::emit(&p, &a, true)).unwrap();
    let output = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()))
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-fsanitize=address,undefined",
            "-fno-omit-frame-pointer",
        ])
        .arg(cpath)
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
    limits.timeout_ms = 10000;
    // ASan reserves shadow address space beyond ordinary worker limits.
    limits.memory_mib = 0;
    for i in 0..p.tests.len() {
        let (status, error) = run_worker(&binary, i, &limits, None).unwrap();
        assert_eq!(status, "TESTED", "{error:?}");
    }
}

#[test]
fn new_server_apis_preserve_types_effects_and_borrow_rules() {
    for (source, kind) in [
        ("fn f(){json.set(\"{}\",\"\",1)}", "type_mismatch"),
        ("fn f(){json.array_len(1)}", "type_mismatch"),
        ("fn f(){http.header(\"X: a\",true)}", "type_mismatch"),
        (
            "fn f()->Text {match http.query(\"/?x=a\",\"x\") {Ok(v)=>{return v} Err(e)=>{return text.clone(e)}}}",
            "ownership",
        ),
        (
            "fn f(){match json.set(\"{}\",\"\",\"[]\"){Ok(v)=>{}}}",
            "non_exhaustive_match",
        ),
        (
            "fn h(a:read Text,b:read Text,c:read Text)->Text{return \"\"} fn main() effects{net.listen,fs.read}{http.serve_api(8092,\"\",h)}",
            "handler",
        ),
        (
            "fn h(a:read Text,b:read Text,c:read Text,d:take Text)->Text{return d} fn main() effects{net.listen,fs.read}{http.serve_api(8092,\"\",h)}",
            "handler",
        ),
        (
            "fn h(a:read Text,b:read Text,c:read Text,d:read Text)->Text effects{io.stdout}{io.println(c) return \"\"} fn main() effects{net.listen,fs.read}{http.serve_api(8092,\"\",h)}",
            "effect_not_allowed",
        ),
        (
            "fn main(){http.serve_api(8092,\"\",missing)}",
            "effect_not_allowed",
        ),
    ] {
        rejected(source, kind);
    }
    let (p, _) = checked(CATALOG).unwrap();
    assert_eq!(lint::run(CATALOG, &p, true)["status"], "LINTED");
}

#[test]
fn server_data_size_bounds_and_owned_replacement() {
    let source = r#"test "bounds and lifetime" {
        var big = "x"
        var i = 0
        while i < 20 { big = text.concat(big, big) i = i + 1 }
        match http.query(text.concat("/?q=", big), "q") { Ok(_v)=>{assert false} Err(_e)=>{} }
        match http.header(big, "x") { Ok(_v)=>{assert false} Err(_e)=>{} }
        let replacement = json.quote(big)
        match json.set("null", "", replacement) { Ok(_v)=>{assert false} Err(_e)=>{} }
        var half = "x"
        var j = 0
        while j < 19 { half = text.concat(half, half) j = j + 1 }
        let half_json = json.quote(half)
        let container = text.concat(text.concat("{\"a\":", half_json), ",\"b\":0}")
        match json.set(container, "/b", half_json) {
            Ok(_v)=>{assert false}
            Err(e)=>{assert e=="JSON output exceeds 1 MiB"}
        }
        var document = "{\"n\":1}"
        let changed = json.set(document, "/n", "2")
        document = "gone"
        match changed { Ok(v)=>{assert v=="{\"n\":2}"} Err(_e)=>{assert false} }
    }"#;
    let (p, a) = checked(source).unwrap();
    let report = test_engine(source, &p, &a, &["--engine".into(), "both".into()]).unwrap();
    assert_eq!(report["status"], "TESTED", "{report}");
}
