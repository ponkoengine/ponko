use ponko_mcp::{Config, Server};
use serde_json::{Value, json};
use std::path::PathBuf;
fn call(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .handle(&json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params}))
        .unwrap()["result"]
        .clone()
}
#[test]
fn compare_images_is_read_only_local_and_returns_summary_and_new_heatmap() {
    let dir = std::env::temp_dir().join(format!("ponko-image-mcp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut server = Server::new(Config {
        instances_dir: dir.join("none"),
        logs_dir: dir.join("logs"),
        read_only: true,
    });
    let tools = call(&mut server, "tools/list", json!({}));
    let tool = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "compare_images")
        .unwrap();
    assert_eq!(tool["inputSchema"]["required"], json!(["a"]));
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/render");
    let png = std::fs::read_dir(reference)
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "png"))
        .unwrap();
    let heatmap = dir.join("heat.png");
    let args = json!({"a":png,"b":png,"heatmap":heatmap});
    let reply = call(
        &mut server,
        "tools/call",
        json!({"name":"compare_images","arguments":args}),
    );
    assert_eq!(reply["isError"], false, "{reply}");
    let result: Value = serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["within"], true);
    assert_eq!(result["summary"]["ssim"], 1.0);
    assert_eq!(result["heatmap"], json!(heatmap));
    assert!(heatmap.is_file());
    let reply = call(
        &mut server,
        "tools/call",
        json!({"name":"compare_images","arguments":args}),
    );
    assert_eq!(reply["isError"], true, "existing outputs must not be overwritten");
    for args in [
        json!({"a":png,"b":png,"tolerance":256}),
        json!({"a":png,"b":png,"threshold":101}),
        json!({"a":png,"b":png,"edge":17}),
        json!({"a":png,"b":"missing.png"}),
        json!({"a":png}),
    ] {
        assert_eq!(
            call(
                &mut server,
                "tools/call",
                json!({"name":"compare_images","arguments":args})
            )["isError"],
            true
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compare_images_captures_full_size_live_frame_and_removes_temporary_capture() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    let dir = std::env::temp_dir().join(format!("ponko-image-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/render");
    let png = std::fs::read_dir(fixture)
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "png"))
        .unwrap();
    let captured = dir.join("captured.png");
    std::fs::copy(&png, &captured).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let pid = std::process::id();
    std::fs::write(
        dir.join(format!("{pid}-{port}.json")),
        json!({"pid":pid,"port":port,"token":"test","name":"capture fixture"}).to_string(),
    )
    .unwrap();
    let shot_path = captured.clone();
    let worker = std::thread::spawn(move || {
        // Discovery first probes the endpoint without sending a request.
        let _ = listener.accept().unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "screenshot");
        assert_eq!(request["params"]["max_size"], 0);
        writeln!(stream, "{}", json!({"id":1,"result":{"path":shot_path}})).unwrap();
    });
    let mut server = Server::new(Config {
        instances_dir: dir.clone(),
        logs_dir: dir.join("logs"),
        read_only: true,
    });
    let reply = call(
        &mut server,
        "tools/call",
        json!({"name":"compare_images","arguments":{"a":png}}),
    );
    worker.join().unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
    let result: Value = serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["within"], true);
    assert!(!captured.exists());
    std::fs::remove_dir_all(dir).unwrap();
}
