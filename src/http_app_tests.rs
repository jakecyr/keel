use super::*;

const ROUTE: &str = r#"
fn route(method: read Text, path: read Text, body: read Text) -> Text {
    if path == "/api/echo" && method == "POST" {
        match http.json_response(200, body) {
            Ok(response) => { return text.clone(response) }
            Err(_error) => { return http.response(400, "invalid JSON") }
        }
    }
    return http.response(404, "not found")
}
"#;

#[test]
fn application_handler_types_effects_and_ownership() {
    for (source, kind) in [
        (format!("{ROUTE} fn main() {{ http.serve_app(8090, \"public\", route) }}"), "effect_not_allowed"),
        (format!("{ROUTE} fn main() effects {{ net.listen, fs.read }} {{ http.serve_app(8090, \"public\") }}"), "arity"),
        ("fn h(p:read Text)->Text{return \"\"} fn main() effects {net.listen,fs.read} {http.serve_app(8090,\"public\",h)}".into(), "handler"),
        ("fn h(m:take Text,p:read Text,b:read Text)->Text{return m} fn main() effects {net.listen,fs.read} {http.serve_app(8090,\"public\",h)}".into(), "handler"),
        ("fn h(m:read Text,p:read Text,b:read Text)->Text effects {io.stdout} {io.println(p) return \"\"} fn main() effects {net.listen,fs.read} {http.serve_app(8090,\"public\",h)}".into(), "effect_not_allowed"),
        ("fn h(m:read Text,p:read Text,b:read Text)->Text{return b}".into(), "ownership"),
    ] {
        rejected(&source, kind);
    }
    let effects = "fn h(_m:read Text,p:read Text,_b:read Text)->Text effects {io.stdout} {io.println(p) return http.response(200,\"ok\")} fn main() effects {net.listen,fs.read,io.stdout} {http.serve_app(8090,\"public\",h)}";
    let (program, _) = checked(effects).unwrap();
    assert_eq!(lint::run(effects, &program, true)["status"], "LINTED");
}

#[test]
fn app_routes_and_argument_evaluation_match_reference() {
    let source = format!(
        r#"{ROUTE}
fn main() {{}}
test "JSON route" {{
    let r = route("POST", "/api/echo", "{{\"n\":7}}")
    assert http.status(r) == 200
    assert http.body(r) == "{{\"n\":7}}"
    assert http.status(route("POST", "/api/echo", "bad")) == 400
    assert http.status(route("GET", "/", "")) == 404
}}
"#
    );
    let (p, a) = checked(&source).unwrap();
    assert_eq!(
        run_tests(&source, &p, &a, &options()).unwrap()["status"],
        "TESTED"
    );
    assert_eq!(
        eval::run_tests(&source, &p, &options()).unwrap()["status"],
        "TESTED"
    );
    // Both argument expressions must run before the host validates the port.
    let source = format!(
        r#"{ROUTE}
fn bad_root()->Text {{ assert false return "public" }}
fn main() effects {{net.listen,fs.read}} {{http.serve_app(0,bad_root(),route)}}"#
    );
    let (p, a) = checked(&source).unwrap();
    let temp = Temp::new().unwrap();
    let binary = temp.0.join("arguments");
    compile(&p, &a, false, &binary, None).unwrap();
    let output = Command::new(binary).output().unwrap();
    let native: Value = serde_json::from_slice(&output.stderr).unwrap();
    // Normal tests cannot invoke effects. Parse directly to exercise the
    // reference engine's argument evaluation before its host-effect rejection.
    let p = syntax::parse(&format!("{source}\ntest \"arguments\" {{ main() }}")).unwrap();
    let reference = eval::run_case(&p, 0, None, 1000).unwrap_err();
    assert_eq!(native["kind"], "assertion_failure");
    assert_eq!(reference["kind"], native["kind"]);
}

struct AppChild(std::process::Child);
impl Drop for AppChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exchange(port: u16, parts: &[&[u8]]) -> Vec<u8> {
    let combined = parts.concat();
    let mut first = parts[0].to_vec();
    if let Some(offset) = first.windows(17).position(|p| p == b"Host: localhost\r\n") {
        first.splice(
            offset..offset + 17,
            format!("Host: localhost:{port}\r\n").bytes(),
        );
    } else if !combined.windows(5).any(|p| p == b"Host:") {
        let offset = first.windows(2).position(|p| p == b"\r\n").unwrap() + 2;
        first.splice(
            offset..offset,
            format!("Host: localhost:{port}\r\n").bytes(),
        );
    }
    let mut chunks = vec![first.as_slice()];
    chunks.extend_from_slice(&parts[1..]);
    exchange_raw(port, &chunks)
}
fn exchange_raw(port: u16, parts: &[&[u8]]) -> Vec<u8> {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    for part in parts {
        if let Err(error) = socket.write_all(part) {
            // A malformed request can be rejected before its remaining chunks
            // arrive. Still validate the actual response, including for success cases.
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ),
                "{error}"
            );
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    if let Err(error) = socket.shutdown(std::net::Shutdown::Write) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotConnected);
    }
    let mut response = Vec::new();
    if let Err(error) = socket.read_to_end(&mut response) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    assert!(
        !response.is_empty(),
        "server sent no response to {:?}",
        parts.concat()
    );
    response
}
fn body(response: &[u8]) -> &[u8] {
    let offset = response.windows(4).position(|p| p == b"\r\n\r\n").unwrap();
    &response[offset + 4..]
}

#[test]
fn static_assets_and_json_endpoints_under_sanitizers() {
    let temp = Temp::new().unwrap();
    let public = temp.0.join("public");
    fs::create_dir_all(public.join("nested")).unwrap();
    fs::write(public.join("index.html"), "<h1>Keel</h1>").unwrap();
    fs::write(public.join("nested/index.html"), "nested").unwrap();
    fs::write(public.join("style.css"), "body{}").unwrap();
    fs::write(public.join("image.png"), [0, 255, 128, 1]).unwrap();
    fs::write(public.join("space name.txt"), "space").unwrap();
    fs::write(public.join(".env"), "test-secret").unwrap();
    fs::write(temp.0.join("outside"), "outside-secret").unwrap();
    std::os::unix::fs::symlink(temp.0.join("outside"), public.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(&temp.0, public.join("escape")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let source = format!(
        "{ROUTE}\nfn main() effects {{net.listen,fs.read}} {{http.serve_app({port},\"public\",route)}}"
    );
    let (p, a) = checked(&source).unwrap();
    let cpath = temp.0.join("app.c");
    let binary = temp.0.join("app");
    fs::write(&cpath, native::emit(&p, &a, false)).unwrap();
    let compiled = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()))
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
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let permission = format!("--allow-net=127.0.0.1:{port}");
    for (args, expected) in [
        (vec![], "permission_denied_net"),
        (vec![permission.as_str()], "permission_denied_fs"),
    ] {
        let output = Command::new(&binary)
            .args(args)
            .current_dir(&temp.0)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
    let errors = temp.0.join("stderr");
    let mut child = AppChild(
        Command::new(&binary)
            .args([&permission, "--allow-read=public"])
            .current_dir(&temp.0)
            .env("ASAN_OPTIONS", "detect_leaks=0")
            .stdout(Stdio::null())
            .stderr(fs::File::create(&errors).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "{}",
            fs::read_to_string(&errors).unwrap()
        );
        if fs::read_to_string(&errors)
            .unwrap()
            .contains("listening on")
        {
            break;
        }
        assert!(Instant::now() < deadline, "server startup timed out");
        thread::sleep(Duration::from_millis(10));
    }
    for (path, mime, expected) in [
        ("/", "text/html", b"<h1>Keel</h1>".as_slice()),
        ("/style.css?v=1", "text/css", b"body{}".as_slice()),
        ("/nested/", "text/html", b"nested".as_slice()),
        ("/image.png", "image/png", [0, 255, 128, 1].as_slice()),
        ("/space%20name.txt", "text/plain", b"space".as_slice()),
    ] {
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let response = exchange(port, &[request.as_bytes()]);
        assert!(
            response.starts_with(b"HTTP/1.1 200"),
            "{path}: {response:?}"
        );
        assert!(String::from_utf8_lossy(&response).contains(mime));
        assert_eq!(body(&response), expected);
    }
    let head = exchange(
        port,
        &[b"HEAD /image.png HTTP/1.1\r\nHost: localhost\r\n\r\n"],
    );
    assert!(String::from_utf8_lossy(&head).contains("Content-Length: 4\r\n"));
    assert!(body(&head).is_empty());
    for path in [
        "/../outside",
        "/%2e%2e/outside",
        "/.env",
        "/%2eenv",
        "/link.txt",
        "/escape/outside",
        "/%00",
        "/%2fetc",
        "/missing",
    ] {
        let request = format!("GET {path} HTTP/1.1\r\n\r\n");
        let response = exchange(port, &[request.as_bytes()]);
        assert!(
            response.starts_with(b"HTTP/1.1 404"),
            "{path}: {response:?}"
        );
        assert!(!String::from_utf8_lossy(&response).contains("secret"));
    }
    let response = exchange(
        port,
        &[
            b"POST /api/echo HTTP/1.1\r\nContent-Length: 7\r\n\r\n{",
            b"\"n\":7}",
        ],
    );
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(body(&response), b"{\"n\":7}");
    for headers in [
        format!("Host: attacker.example:{port}\r\n"),
        format!("Host: localhost:{port}\r\nOrigin: https://attacker.example\r\n"),
        format!("Host: localhost:{port}\r\nSec-Fetch-Site: cross-site\r\n"),
        String::new(),
    ] {
        let request = format!(
            "POST /api/echo HTTP/1.1\r\n{headers}Content-Type: text/plain\r\nContent-Length: 2\r\n\r\n{{}}"
        );
        assert!(exchange_raw(port, &[request.as_bytes()]).starts_with(b"HTTP/1.1 403"));
    }
    let same_origin = format!(
        "POST /api/echo HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    assert!(exchange_raw(port, &[same_origin.as_bytes()]).starts_with(b"HTTP/1.1 200"));
    for malformed in [
        b"POST /api/echo HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
        b"POST /api/echo HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
        b"POST /api/echo HTTP/1.1\r\nContent-Length: 9\r\n\r\n{}".as_slice(),
        b"POST /api/echo HTTP/1.1\r\nContent-Length: 1\r\n\r\n\xff".as_slice(),
        b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n".as_slice(),
    ] {
        assert!(exchange(port, &[malformed]).starts_with(b"HTTP/1.1 400"));
    }
    assert!(
        exchange(
            port,
            &[b"POST /api/echo HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n"]
        )
        .starts_with(b"HTTP/1.1 413")
    );
    let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    slow.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let mut timed_out = Vec::new();
    slow.read_to_end(&mut timed_out).unwrap();
    assert!(timed_out.starts_with(b"HTTP/1.1 408"));
    drop(child);
    let diagnostics = fs::read_to_string(errors).unwrap();
    assert!(
        !diagnostics.contains("ERROR: AddressSanitizer"),
        "{diagnostics}"
    );
    assert!(!diagnostics.contains("runtime error:"), "{diagnostics}");
}

#[test]
fn explicit_http_worker_deadline_is_bounded_and_recoverable() {
    rejected(
        "fn f(){http.post_json_timeout(\"http://127.0.0.1:1\",\"{}\",\"\",50)}",
        "effect_not_allowed",
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let source = format!(
        r#"
fn main() effects {{net.connect}} {{
    match http.post_json_timeout("http://127.0.0.1:{port}", "{{}}", "", 0) {{Ok(v)=>{{assert false}} Err(e)=>{{assert text.len(e)>0}}}}
    match http.post_json_timeout("http://127.0.0.1:{port}", "{{}}", "", 120001) {{Ok(v)=>{{assert false}} Err(e)=>{{assert text.len(e)>0}}}}
    match http.post_json_timeout("http://127.0.0.1:{port}", "{{}}", "", 50) {{Ok(v)=>{{assert false}} Err(e)=>{{assert text.len(e)>0}}}}
}}"#
    );
    let (p, a) = checked(&source).unwrap();
    let temp = Temp::new().unwrap();
    let binary = temp.0.join("deadline");
    compile(&p, &a, false, &binary, None).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok((mut socket, _)) = listener.accept() {
                thread::sleep(Duration::from_millis(200));
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                );
                drop(socket);
                return;
            }
            assert!(
                Instant::now() < deadline,
                "no request reached the mock server"
            );
            thread::sleep(Duration::from_millis(5));
        }
    });
    let output = Command::new(binary)
        .arg(format!("--allow-connect=http://127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
}

fn sanitized_server(source: &str, temp: &Temp) -> PathBuf {
    let (p, a) = checked(source).unwrap();
    let cpath = temp.0.join("host.c");
    let binary = temp.0.join("host");
    fs::write(&cpath, native::emit(&p, &a, false)).unwrap();
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
    binary
}
struct ObservedServer {
    child: AppChild,
    errors: PathBuf,
}
impl Drop for ObservedServer {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!(
                "server status: {:?}; stderr: {}",
                self.child.0.try_wait(),
                fs::read_to_string(&self.errors).unwrap_or_default()
            );
        }
    }
}
fn start_server(binary: &Path, port: u16, temp: &Temp) -> ObservedServer {
    let errors = temp.0.join("stderr");
    let mut child = AppChild(
        Command::new(binary)
            .arg(format!("--allow-net=127.0.0.1:{port}"))
            .current_dir(&temp.0)
            .env("ASAN_OPTIONS", "detect_leaks=0")
            .stdout(Stdio::null())
            .stderr(fs::File::create(&errors).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "{}",
            fs::read_to_string(&errors).unwrap()
        );
        if fs::read_to_string(&errors)
            .unwrap()
            .contains("listening on")
        {
            break;
        }
        assert!(Instant::now() < deadline, "server startup timed out");
        thread::sleep(Duration::from_millis(10));
    }
    ObservedServer { child, errors }
}
fn slow_request_is_bounded(port: u16) {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(7)))
        .unwrap();
    socket.write_all(b"GET /").unwrap();
    let mut writer = socket.try_clone().unwrap();
    let trickle = thread::spawn(move || {
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(100));
            if writer.write_all(b"x").is_err() {
                return;
            }
        }
        let _ = writer.shutdown(std::net::Shutdown::Write);
    });
    let start = Instant::now();
    let mut response = Vec::new();
    let read = socket.read_to_end(&mut response);
    if let Err(e) = read {
        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
    }
    let elapsed = start.elapsed();
    trickle.join().unwrap();
    assert!(
        elapsed < Duration::from_millis(3500),
        "slow client monopolized server for {elapsed:?}"
    );
    assert!(response.starts_with(b"HTTP/1.1 400") || response.starts_with(b"HTTP/1.1 408"));
}

#[test]
fn catalog_example_real_http_and_adversarial_requests_under_sanitizers() {
    let temp = Temp::new().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let source = format!(
        "{}\n{}",
        include_str!("../examples/catalog_api/main.keel").replace("8092", &port.to_string()),
        include_str!("../examples/catalog_api/routes.keel")
    );
    let binary = sanitized_server(&source, &temp);
    let denied = Command::new(&binary).output().unwrap();
    assert!(String::from_utf8_lossy(&denied.stderr).contains("permission_denied_net"));
    drop(listener);
    let _child = start_server(&binary, port, &temp);
    let data = "[{\"name\":\"tea 🍵\",\"price_cents\":199,\"score\":0.9900}]";
    let header = format!(
        "POST /api/discount?percent=%31%30 HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        data.len()
    );
    let response = exchange(
        port,
        &[
            header.as_bytes(),
            &data.as_bytes()[..10],
            &data.as_bytes()[10..],
        ],
    );
    assert!(
        response.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&response)
    );
    assert_eq!(
        body(&response),
        "[{\"name\":\"tea 🍵\",\"price_cents\":180,\"score\":0.9900}]".as_bytes()
    );
    for (request, status) in [
        (b"POST /api/discount?percent=10&percent=20 HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]".as_slice(), 400),
        (b"POST /api/discount?percent=10 HTTP/1.1\r\nContent-Type: application/json\r\ncontent-type: text/plain\r\nContent-Length: 2\r\n\r\n[]", 400),
        (b"GET /api/health?x=\xff HTTP/1.1\r\n\r\n", 400),
        (b"GET /api/health HTTP/1.1\r\nX-Test: \xff\r\n\r\n", 400),
        (b"GET /api/health#fragment HTTP/1.1\r\n\r\n", 400),
        (b"POST /api/discount?percent=10 HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n", 400),
        (b"POST /api/discount?percent=10 HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n", 400),
        (b"POST /api/discount?percent=10 HTTP/1.1\r\nContent-Length: 1048577\r\n\r\n", 413),
        (b"POST /api/discount?percent=10 HTTP/1.1\r\nExpect: 100-continue\r\n\r\n", 417),
    ] {
        let response = exchange(port, &[request]);
        assert!(response.starts_with(format!("HTTP/1.1 {status}").as_bytes()), "{}", String::from_utf8_lossy(&response));
    }
    slow_request_is_bounded(port);
    assert!(exchange(port, &[b"GET /api/health HTTP/1.1\r\n\r\n"]).starts_with(b"HTTP/1.1 200"));
    let stderr = fs::read_to_string(temp.0.join("stderr")).unwrap();
    assert!(
        !stderr.contains("Sanitizer") && !stderr.contains("runtime error:"),
        "{stderr}"
    );
}

#[test]
fn legacy_host_rejects_invalid_text_and_bounds_trickle_clients() {
    let temp = Temp::new().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let source = format!(
        "fn route(path:read Text)->Text{{return http.response(200,path)}} fn main() effects{{net.listen}}{{http.serve({port},route)}}"
    );
    let binary = sanitized_server(&source, &temp);
    drop(listener);
    let _child = start_server(&binary, port, &temp);
    for request in [
        b"GET /\xff HTTP/1.1\r\n\r\n".as_slice(),
        b"GET /x\t HTTP/1.1\r\n\r\n",
        b"GET /x#fragment HTTP/1.1\r\n\r\n",
    ] {
        assert!(exchange_raw(port, &[request]).starts_with(b"HTTP/1.1 400"));
    }
    slow_request_is_bounded(port);
    assert_eq!(
        body(&exchange_raw(port, &[b"GET /ok HTTP/1.1\r\n\r\n"])),
        b"/ok"
    );
    let stderr = fs::read_to_string(temp.0.join("stderr")).unwrap();
    assert!(
        !stderr.contains("Sanitizer") && !stderr.contains("runtime error:"),
        "{stderr}"
    );
}

#[test]
fn response_backpressure_obeys_total_deadline_under_sanitizers() {
    let temp = Temp::new().unwrap();
    let cpath = temp.0.join("backpressure.c");
    let binary = temp.0.join("backpressure");
    let harness = r#"
int main(void) {
  int sockets[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockets)) return 1;
  int size = 1024;
  if (setsockopt(sockets[0], SOL_SOCKET, SO_SNDBUF, &size, sizeof(size))) return 2;
  int flags = fcntl(sockets[0], F_GETFL, 0);
  if (flags < 0 || fcntl(sockets[0], F_SETFL, flags | O_NONBLOCK)) return 3;
  KText payload = k_alloc(1024 * 1024);
  memset((char *)payload.ptr, 'x', payload.len);
  int64_t start = k_millis();
  bool success = k_send_all(sockets[0], payload);
  int64_t elapsed = k_millis() - start;
  k_drop(&payload); close(sockets[0]); close(sockets[1]);
  return !success && elapsed >= 1900 && elapsed < 3500 ? 0 : 4;
}
"#;
    fs::write(
        &cpath,
        format!(
            "{}\n{}\n{harness}",
            include_str!("runtime.c"),
            include_str!("stdlib.c")
        ),
    )
    .unwrap();
    let output = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()))
        .args([
            "-std=c11",
            "-O1",
            "-g",
            "-fsanitize=address,undefined",
            "-fno-omit-frame-pointer",
            "-pthread",
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
    // ASan reserves a large virtual address range. Keep the execution deadline,
    // but do not constrain its address space with the ordinary worker RLIMIT_AS.
    let output = process::capture(&mut Command::new(binary), 6000, 0).unwrap();
    assert!(
        !output.timed_out && output.status.success(),
        "{}",
        output.stderr
    );
    assert!(!output.stderr.contains("Sanitizer") && !output.stderr.contains("runtime error:"));
}
