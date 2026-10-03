use super::*;
use std::net::UdpSocket;

const STANDARD: &str = include_str!("../examples/stdlib.keel");
#[test]
fn standard_library_native_and_reference() {
    let (program, analysis) = checked(STANDARD).unwrap();
    let native = run_tests(STANDARD, &program, &analysis, &options()).unwrap();
    let reference = eval::run_tests(STANDARD, &program, &options()).unwrap();
    assert_eq!(native["status"], "TESTED", "{native}");
    assert_eq!(reference["status"], "TESTED", "{reference}");
}
#[test]
fn standard_library_rejects_ownership_effect_and_worker_errors() {
    for (source, kind) in [
        ("fn f() { let a=json.parse(\"{}\") let b=a }", "ownership"),
        (
            "fn f() -> Text { match json.parse(\"{}\") { Ok(x)=>{return x} Err(e)=>{return text.clone(e)} } }",
            "ownership",
        ),
        (
            "fn f() { var a=json.parse(\"{}\") match a { Ok(x)=>{a=json.parse(\"[]\")} Err(e)=>{} } }",
            "borrow_conflict",
        ),
        (
            "fn f() { match json.parse(\"{}\") { Ok(x)=>{} } }",
            "non_exhaustive_match",
        ),
        ("fn f() { fs.read_text(\"secret\") }", "effect_not_allowed"),
        ("fn f() { env.get(\"SECRET\") }", "effect_not_allowed"),
        (
            "fn f() { http.get(\"https://example.com\") }",
            "effect_not_allowed",
        ),
        (
            "fn f() { tcp.exchange(\"127.0.0.1\",80,\"\") }",
            "effect_not_allowed",
        ),
        (
            "fn worker(n:Int)->Int effects { io.stdout } { io.println(\"bad\") return n } fn f() { parallel.map([1],worker) }",
            "handler",
        ),
        (
            "fn worker(n:read Text)->Int { return 0 } fn f() { parallel.map([1],worker) }",
            "handler",
        ),
    ] {
        rejected(source, kind);
    }
}
fn application(source: &str, temp: &Temp) -> PathBuf {
    let (program, analysis) = checked(source).unwrap();
    let binary = temp.0.join("app");
    compile(&program, &analysis, false, &binary, None).unwrap();
    binary
}
#[test]
fn standard_library_under_sanitizers() {
    let (program, analysis) = checked(STANDARD).unwrap();
    let temp = Temp::new().unwrap();
    let cpath = temp.0.join("std.c");
    let binary = temp.0.join("std");
    fs::write(&cpath, native::emit(&program, &analysis, true)).unwrap();
    let mut compiler = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()));
    compiler
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-fsanitize=address,undefined",
            "-fno-omit-frame-pointer",
        ])
        .arg(cpath)
        .arg("-o")
        .arg(&binary);
    native_libraries(&mut compiler, &analysis).unwrap();
    let output = compiler.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut limits = options();
    limits.timeout_ms = 10000;
    // ASan reserves shadow address space beyond ordinary worker limits.
    limits.memory_mib = 0;
    for i in 0..program.tests.len() {
        let (status, error) = run_worker(&binary, i, &limits, None).unwrap();
        assert_eq!(status, "TESTED", "{error:?}");
    }
}
#[test]
fn files_environment_permissions_and_utf8() {
    let temp = Temp::new().unwrap();
    let path = temp.0.join("input");
    fs::write(&path, "hello 😀").unwrap();
    let path_literal = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let source = format!(
        "fn main() effects {{ fs.read }} {{ match fs.read_text({path_literal}) {{ Ok(s)=>{{assert s==\"hello 😀\"}} Err(e)=>{{assert false}} }} }}"
    );
    let binary = application(&source, &temp);
    let denied = Command::new(&binary).output().unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("permission_denied_fs"));
    assert!(
        Command::new(&binary)
            .arg(format!("--allow-read={}", path.display()))
            .status()
            .unwrap()
            .success()
    );
    fs::write(&path, [0xff]).unwrap();
    assert!(
        !Command::new(&binary)
            .arg(format!("--allow-read={}", path.display()))
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", &path).unwrap();
    assert!(
        !Command::new(&binary)
            .arg(format!("--allow-read={}", path.display()))
            .output()
            .unwrap()
            .status
            .success()
    );
    let binary = application(
        "fn main() effects { env.read } { match env.get(\"KEEL_STD_TEST\") { Ok(value)=>{assert value==\"present\"} Err(e)=>{assert false} } }",
        &temp,
    );
    assert!(
        Command::new(&binary)
            .arg("--allow-env=KEEL_STD_TEST")
            .env("KEEL_STD_TEST", "present")
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !Command::new(&binary)
            .env("KEEL_STD_TEST", "present")
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn tcp_and_udp_loopback_exchange() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = String::new();
        socket.read_to_string(&mut request).unwrap();
        assert_eq!(request, "ping");
        socket.write_all("pong 😀".as_bytes()).unwrap();
    });
    let temp = Temp::new().unwrap();
    let binary = application(
        &format!(
            "fn main() effects {{ net.connect }} {{ match tcp.exchange(\"127.0.0.1\",{port},\"ping\") {{ Ok(reply)=>{{assert reply==\"pong 😀\"}} Err(e)=>{{assert false}} }} }}"
        ),
        &temp,
    );
    assert!(!Command::new(&binary).output().unwrap().status.success());
    assert!(
        Command::new(&binary)
            .arg(format!("--allow-connect=tcp://127.0.0.1:{port}"))
            .status()
            .unwrap()
            .success()
    );
    worker.join().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let port = socket.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let mut buffer = [0; 64];
        let (n, peer) = socket.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"ping");
        socket.send_to(b"pong", peer).unwrap();
    });
    let binary = application(
        &format!(
            "fn main() effects {{ net.connect }} {{ match udp.exchange(\"127.0.0.1\",{port},\"ping\") {{ Ok(reply)=>{{assert reply==\"pong\"}} Err(e)=>{{assert false}} }} }}"
        ),
        &temp,
    );
    assert!(
        Command::new(&binary)
            .arg(format!("--allow-connect=udp://127.0.0.1:{port}"))
            .status()
            .unwrap()
            .success()
    );
    worker.join().unwrap();
}
#[test]
fn http_json_loopback_status_auth_and_no_redirect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        for index in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            let headers = String::from_utf8(request).unwrap();
            if index == 0 {
                assert!(headers.starts_with("POST /query HTTP/1.1"));
                assert!(headers.contains("Authorization: Bearer test-only"));
                assert!(headers.contains("Content-Type: application/json"));
                let mut body = [0; 7];
                socket.read_exact(&mut body).unwrap();
                assert_eq!(&body, b"{\"n\":1}");
                socket.write_all(b"HTTP/1.1 422 Unprocessable Entity\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"ok\":false}").unwrap();
            } else {
                socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        }
    });
    let temp = Temp::new().unwrap();
    let binary = application(
        &format!(
            r#"fn main() effects {{ net.connect }} {{
        match http.post_json("http://127.0.0.1:{port}/query", "{{\"n\":1}}", "test-only") {{
            Ok(response)=>{{assert http.status(response)==422 assert http.body(response)=="{{\"ok\":false}}"}} Err(e)=>{{assert false}}
        }}
        match http.get("http://127.0.0.1:{port}/redirect") {{ Ok(response)=>{{assert http.status(response)==302}} Err(e)=>{{assert false}} }}
    }}"#
        ),
        &temp,
    );
    let output = Command::new(&binary)
        .arg(format!("--allow-connect=http://127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    worker.join().unwrap();
}
#[test]
fn parser_adversarial_differential_corpus() {
    let mut source = String::from("fn main() {} test \"corpus\" {\n");
    let mut cases = vec![
        "{\"a\":\"\\ud800\",\"a\":1}".to_string(),
        "\"\\u0000\"".into(),
        "{\"\\ud800\":1}".into(),
        "-0.3E-999".into(),
        "[true,false,null]".into(),
        "{\"\":[]}".into(),
    ];
    for depth in [63, 64, 65, 128] {
        cases.push(format!("{}0{}", "[".repeat(depth), "]".repeat(depth)));
    }
    for text in [
        "null",
        "[1,2]",
        "{\"a\":true}",
        "\"escaped\\ntext\"",
        "1.2e-3",
    ] {
        for len in 0..text.len() {
            cases.push(text[..len].into());
        }
    }
    for raw in cases {
        let expected = crate::stdlib::json_parse(&raw);
        source.push_str(&format!(
            "match json.parse({}) {{ Ok(v)=>{{assert {}}} Err(e)=>{{assert {}}} }}\n",
            serde_json::to_string(&raw).unwrap(),
            expected.is_ok(),
            expected.is_err()
        ));
    }
    source.push('}');
    let native = evaluate(&source);
    assert_eq!(native["status"], "TESTED", "{native}");
}

#[test]
fn websocket_capability_and_loopback() {
    let temp = Temp::new().unwrap();
    let (_, analysis) = checked(
        "fn main() effects { net.connect } { websocket.exchange(\"ws://127.0.0.1:1/\",\"ping\") }",
    )
    .unwrap();
    let probe = temp.0.join("probe.c");
    let binary = temp.0.join("probe");
    fs::write(&probe,"#include <curl/curl.h>\n#include <string.h>\nint main(void) { if(curl_version_info(CURLVERSION_NOW)->version_num < 0x081000) return 1; const char *const *p=curl_version_info(CURLVERSION_NOW)->protocols;for(;*p;p++)if(!strcmp(*p,\"ws\"))return 0;return 1; }\n").unwrap();
    let mut compiler = Command::new("cc");
    compiler.arg(&probe).arg("-o").arg(&binary);
    native_libraries(&mut compiler, &analysis).unwrap();
    assert!(compiler.status().unwrap().success());
    if !Command::new(&binary).status().unwrap().success() {
        assert!(
            env::var_os("KEEL_REQUIRE_WEBSOCKET_TEST").is_none(),
            "WebSocket wire coverage required, but libcurl lacks ws or is older than 8.16"
        );
        let app = application(
            "fn main() effects { net.connect } { match websocket.exchange(\"ws://127.0.0.1:1/\",\"ping\") { Ok(v)=>{assert false} Err(e)=>{assert text.len(e)>0} } }",
            &temp,
        );
        assert!(
            Command::new(app)
                .arg("--allow-connect=ws://127.0.0.1:1")
                .status()
                .unwrap()
                .success()
        );
        eprintln!(
            "WebSocket wire test unavailable: host libcurl lacks ws or is older than 8.16; recoverable unsupported path tested."
        );
        return;
    }
    use std::io::BufRead;
    let mut server = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/websocket_server.py"
        ))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = std::io::BufReader::new(server.stdout.take().unwrap());
    let mut port = String::new();
    reader.read_line(&mut port).unwrap();
    let port: u16 = port.trim().parse().unwrap();
    let binary = application(
        &format!(
            "fn main() effects {{ net.connect }} {{ match websocket.exchange(\"ws://127.0.0.1:{port}/\",\"ping\") {{ Ok(v)=>{{assert v==\"pong\"}} Err(e)=>{{assert false}} }} }}"
        ),
        &temp,
    );
    let output = Command::new(binary)
        .arg(format!("--allow-connect=ws://127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(server.wait().unwrap().success());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn standard_resource_limits_remain_unknown() {
    let source = r#"test "quote bound" {
        var value="ab"
        var index=0
        while index<20 { value=text.concat(value,value) index=index+1 }
        json.quote(value)
    }"#;
    let (program, analysis) = checked(source).unwrap();
    let report = test_engine(
        source,
        &program,
        &analysis,
        &["--engine".into(), "both".into()],
    )
    .unwrap();
    assert_eq!(report["status"], "UNKNOWN", "{report}");
    assert_eq!(report["differential"]["mismatches"], json!([]), "{report}");
}
