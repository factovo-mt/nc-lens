use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    time::Duration,
};

use serde_json::{json, Value};

struct Server {
    child: Child,
    addr: SocketAddr,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nclens-server"))
            .args(["--port", "0", "--max-body", "1024", "--timeout", "1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(stdout).read_line(&mut line).unwrap();
            let _ = tx.send(line);
        });
        let mut server = Self {
            child,
            addr: "127.0.0.1:0".parse().unwrap(),
        };
        let line = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        server.addr = line
            .trim()
            .strip_prefix("Listening on ")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(server.addr.ip().to_string(), "127.0.0.1");
        server
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        headers: &str,
        body: &[u8],
    ) -> (u16, String, String) {
        let mut stream = self.connect();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n", self.addr, body.len()).unwrap();
        stream.write_all(body).unwrap();
        response(stream)
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect_timeout(&self.addr, Duration::from_secs(4)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn response(mut stream: TcpStream) -> (u16, String, String) {
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let code = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, headers.to_ascii_lowercase(), body.to_owned())
}

#[test]
fn http_endpoints_and_limits() {
    let server = Server::start();
    let (code, headers, body) = server.request("GET", "/v1/health", "", b"");
    assert_eq!(code, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["status"],
        "ok"
    );
    assert!(!headers.contains("access-control-allow-origin"));
    let (code, _, body) = server.request("GET", "/v1/mnemonics", "", b"");
    assert_eq!(code, 200);
    assert!(body.contains("fanuc") && body.contains("osp"));

    let (code, _, body) = server.request(
        "POST",
        "/v1/macro-reference",
        "Content-Type: application/json\r\n",
        br#"{"macro":{"comparisons":[{"symbol":">=","operation":"ge","precedence":3}]}}"#,
    );
    assert_eq!(code, 200, "{body}");
    let reference: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reference["comparisons"].as_array().unwrap().len(), 1);
    assert_eq!(reference["comparisons"][0]["symbol"], ">=");
    let (code, _, body) = server.request(
        "POST",
        "/v1/macro-reference",
        "Content-Type: application/json\r\n",
        b"{\n  \"macro\": {\n    \"expression\": [\"(\", \"]\"]\n  }\n}",
    );
    assert_eq!(code, 400, "{body}");
    assert!(body.contains("line 3"), "{body}");

    let nc = "O1\nG0 X5.\nM30\n";
    let data = json!({
        "nc": nc, "encoding": "utf-8", "include": ["expanded", "trace", "path"],
        "post": "name = 'Integration'\n", "set": {"variables.work.G54.x": 10},
    });
    let (code, _, body) = server.request(
        "POST",
        "/v1/run",
        "Content-Type: application/json\r\n",
        data.to_string().as_bytes(),
    );
    assert_eq!(code, 200, "{body}");
    let result: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["post"]["name"], "Integration");
    assert_eq!(result["path"][0]["to"][0], 15.);
    assert!(result["trace"].is_array());
    assert!(result.get("report").is_none());

    let (code, headers, body) = server.request(
        "POST",
        "/v1/expand",
        "Content-Type: text/plain; charset=cp932\r\n",
        b"(\x8c\x8a)\nG0 X5.\nM30\n",
    );
    assert_eq!(code, 200, "{body}");
    assert!(headers.contains("content-type: text/plain"));
    assert!(body.contains("X5"));
    let (code, _, body) = server.request("POST", "/v1/report", "", nc.as_bytes());
    assert_eq!(code, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["errors"], 0);
    let (code, _, body) = server.request("POST", "/v1/run", "", b"#1=[\n");
    assert_eq!(code, 422);
    assert!(
        !serde_json::from_str::<Value>(&body).unwrap()["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        server
            .request("POST", "/v1/run", "Origin: http://example.test\r\n", b"X1")
            .0,
        403
    );
    assert_eq!(server.request("POST", "/v1/run", "", &[b'X'; 1025]).0, 413);
    assert_eq!(
        server
            .request(
                "POST",
                "/v1/run",
                "Content-Type: application/json\r\n",
                b"{}"
            )
            .0,
        400
    );
    assert_eq!(
        server
            .request("POST", "/v1/run", "Content-Type: application/xml\r\n", b"x")
            .0,
        415
    );

    let mut stream = server.connect();
    write!(
        stream,
        "POST /v1/run HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: 100\r\n\r\nX",
        server.addr
    )
    .unwrap();
    assert_eq!(response(stream).0, 408);
}

#[test]
fn http_accepts_numbered_block_skip_and_optional_stop() {
    let server = Server::start();
    let nc = "/G0 X1.\n/2 G0 X2.\n/3 G0 X3.\nM01\nG0 X4.\n";
    let data = json!({
        "nc": nc, "block_skip_levels": [1, 3], "optional_stop": true, "include": ["path"],
    });
    let (code, _, body) = server.request(
        "POST",
        "/v1/run",
        "Content-Type: application/json\r\n",
        data.to_string().as_bytes(),
    );
    assert_eq!(code, 200, "{body}");
    let result: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["path"].as_array().unwrap().len(), 1);
    assert_eq!(result["path"][0]["src_line"], 2);
    assert_eq!(
        result["block_skip_usage"],
        json!([
            {"level": 1, "line_count": 1},
            {"level": 2, "line_count": 1},
            {"level": 3, "line_count": 1},
        ])
    );
}
