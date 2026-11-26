use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderValue, StatusCode};
use tungstenite::stream::MaybeTlsStream;

struct Bridge(Child);

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn browser_bridge_registers_and_translates_the_existing_control_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let mut bridge = Bridge(
        Command::new(env!("CARGO_BIN_EXE_ponko-mcp"))
            .arg("--web-bridge")
            .env("PONKO_INSTANCES_DIR", dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(bridge.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line.trim().strip_prefix("#ponko-control=").expect("bridge URL");
    let request = |url: &str, origin: &'static str| {
        let mut req = url.into_client_request().unwrap();
        req.headers_mut().insert("origin", HeaderValue::from_static(origin));
        req
    };
    let rejected = |url, origin| {
        assert!(matches!(
            tungstenite::connect(request(url, origin)),
            Err(tungstenite::Error::Http(response)) if response.status() == StatusCode::FORBIDDEN
        ));
    };
    rejected(url, "https://example.com");
    let wrong_url = format!("{}/wrong", url.rsplit_once('/').unwrap().0);
    rejected(&wrong_url, "http://localhost:8000");
    let (mut browser, _) = tungstenite::connect(request(url, "http://localhost:8000")).unwrap();
    if let MaybeTlsStream::Plain(socket) = browser.get_ref() {
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let (entry, info) = loop {
        if let Some(entry) = std::fs::read_dir(dir.path()).unwrap().flatten().next()
            && let Ok(text) = std::fs::read_to_string(entry.path())
            && let Ok(info) = serde_json::from_str::<Value>(&text)
        {
            break (entry.path(), info);
        }
        assert!(Instant::now() < deadline, "browser was not registered");
        std::thread::sleep(Duration::from_millis(10));
    };
    let port = info["port"].as_u64().unwrap() as u16;
    assert_eq!(info["transport"], "web");
    let call = |id, token: &Value, method, params| {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        writeln!(
            stream,
            "{}",
            json!({"id":id,"token":token,"method":method,"params":params})
        )
        .unwrap();
        stream
    };
    let reply = |stream| {
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    // Discovery probes only open and close the endpoint.
    drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
    assert!(reply(call(1, &json!("wrong"), "info", json!({})))["error"].is_string());
    let token = &info["token"];
    let step = call(2, token, "sim.step", json!({"ticks":3}));
    let request: Value = serde_json::from_str(browser.read().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(request, json!({"id":2,"method":"sim.step","params":{"ticks":3}}));
    browser
        .send(json!({"id":2,"result":{"queued_ticks":3}}).to_string().into())
        .unwrap();
    assert_eq!(reply(step), json!({"id":2,"result":{"queued_ticks":3}}));

    let shot = call(3, token, "screenshot", json!({"max_size":1}));
    let request: Value = serde_json::from_str(browser.read().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(request, json!({"id":3,"method":"screenshot","params":{"max_size":1}}));
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aHM8AAAAASUVORK5CYII=";
    browser
        .send(
            json!({"id":3,"result":{"png":png,"path":"/ignored.png"}})
                .to_string()
                .into(),
        )
        .unwrap();
    let result = reply(shot);
    let path = result["result"]["path"].as_str().expect("bridge saved PNG");
    assert_ne!(path, "/ignored.png");
    assert_eq!(result["result"]["width"], 1);
    assert_eq!(result["result"]["height"], 1);
    assert_eq!(data_encoding::BASE64.encode(&std::fs::read(path).unwrap()), png);
    std::fs::remove_file(path).unwrap();
    assert!(reply(call(4, token, "screenshot", json!({"path":"other.png"})))["error"].is_string());

    browser.close(None).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while entry.exists() {
        assert!(Instant::now() < deadline, "disconnected browser remained registered");
        std::thread::sleep(Duration::from_millis(10));
    }
}
