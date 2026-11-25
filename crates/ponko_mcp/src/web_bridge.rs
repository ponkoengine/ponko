//! An opt-in loopback WebSocket, with a native control endpoint for the
//! connected browser tab. Every WebSocket message carries one control JSON
//! object. Only the bridge handles credentials and local screenshot files.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::StatusCode;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

const MAX_REQUEST: usize = 4 * 1024 * 1024;
const MAX_REPLY: usize = 32 * 1024 * 1024;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const TAB_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn run(instances_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let route = format!("/{}", token()?);
    println!("#ponko-control=ws://{}{route}", listener.local_addr()?);
    io::stdout().flush()?;
    for connection in listener.incoming() {
        let stream = connection?;
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_REPLY))
            .max_frame_size(Some(MAX_REPLY));
        #[expect(
            clippy::result_large_err,
            reason = "tungstenite's handshake callback requires ErrorResponse by value"
        )]
        let Ok(socket) = tungstenite::accept_hdr_with_config(
            stream,
            |req: &Request, response: Response| {
                if req.uri().path_and_query().is_some_and(|p| p.as_str() == route) && local_origin(req) {
                    Ok(response)
                } else {
                    let mut error = ErrorResponse::new(Some("a local page and the bridge URL are required".into()));
                    *error.status_mut() = StatusCode::FORBIDDEN;
                    Err(error)
                }
            },
            Some(config),
        ) else {
            continue;
        };
        if let Err(e) = serve_tab(socket, instances_dir) {
            eprintln!("ponko-mcp: browser bridge: {e}");
        }
    }
    Ok(())
}

fn local_origin(req: &Request) -> bool {
    let origin = req
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<tungstenite::http::Uri>().ok());
    origin.is_some_and(|uri| {
        matches!(uri.scheme_str(), Some("http" | "https"))
            && matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]"))
            && uri.path_and_query().is_some_and(|p| p.as_str() == "/")
    })
}

fn token() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(data_encoding::HEXLOWER.encode(&bytes))
}

struct Registration(PathBuf);

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn register(dir: &Path, port: u16, token: &str) -> io::Result<Registration> {
    std::fs::create_dir_all(dir)?;
    let pid = std::process::id();
    let info = json!({
        "name": "Browser game",
        "pid": pid,
        "port": port,
        "token": token,
        "headless": false,
        "transport": "web",
        "started": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
    });
    // Publish the complete entry at once, with the temp file's private permissions.
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(file.as_file_mut(), &info)?;
    let path = dir.join(format!("{pid}-{port}.json"));
    file.persist_noclobber(&path).map_err(|e| e.error)?;
    Ok(Registration(path))
}

fn serve_tab(mut socket: WebSocket<TcpStream>, dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    endpoint.set_nonblocking(true)?;
    let token = token()?;
    let _registration = register(dir, endpoint.local_addr()?.port(), &token)?;
    loop {
        // Notice a closed tab even when no tools are calling it.
        socket.get_ref().set_nonblocking(true)?;
        match socket.read() {
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {}
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            _ => return Ok(()),
        }
        match endpoint.accept() {
            Ok((stream, _)) => {
                socket.get_ref().set_nonblocking(false)?;
                if !serve_request(stream, &token, &mut socket) {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.into()),
        }
    }
}

/// Returns false when the tab disconnected or stopped replying. Retire
/// that session so a late response can never be used for a subsequent call.
fn serve_request(mut stream: TcpStream, token: &str, socket: &mut WebSocket<TcpStream>) -> bool {
    let read = || -> io::Result<Option<Value>> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let mut line = String::new();
        let n = BufReader::new(&stream)
            .take(MAX_REQUEST as u64 + 1)
            .read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // Instance discovery only opened the socket.
        }
        if n > MAX_REQUEST || !line.ends_with('\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid control request line",
            ));
        }
        Ok(Some(serde_json::from_str(&line)?))
    };
    let req = match read() {
        Ok(Some(req)) => req,
        Ok(None) => return true,
        Err(e) => {
            let _ = writeln!(stream, "{}", json!({"id":null,"error":e.to_string()}));
            return true;
        }
    };
    let id = &req["id"];
    let error = |error: &str| json!({"id":id,"error":error});
    let (reply, connected) = if req["token"].as_str() != Some(token) {
        (error("missing or wrong token (see the instance's registry file)"), true)
    } else if req["method"] == "screenshot" && !req["params"]["path"].is_null() {
        (
            error("browser screenshots are saved to a temporary file by the bridge; omit path"),
            true,
        )
    } else {
        let msg = json!({"id":id,"method":req["method"],"params":req["params"]});
        match exchange(socket, &msg) {
            Ok(reply) if req["method"] == "screenshot" && reply.get("result").is_some() => {
                let reply = match save_screenshot(&reply["result"]) {
                    Ok(shot) => json!({"id":id,"result":shot}),
                    Err(e) => error(&e.to_string()),
                };
                (reply, true)
            }
            Ok(reply) => (reply, true),
            Err(e) => (error(&e.to_string()), false),
        }
    };
    let _ = writeln!(stream, "{reply}");
    connected
}

fn exchange(socket: &mut WebSocket<TcpStream>, msg: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    socket.send(Message::Text(msg.to_string().into()))?;
    let deadline = Instant::now() + TAB_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("the browser did not answer in time".into());
        }
        socket.get_ref().set_read_timeout(Some(remaining))?;
        match socket.read()? {
            Message::Text(reply) => {
                let reply: Value = serde_json::from_str(&reply)?;
                if reply.get("id") == msg.get("id") && (reply.get("result").is_some() ^ reply["error"].is_string()) {
                    return Ok(reply);
                }
                return Err("invalid control reply from the browser".into());
            }
            Message::Ping(_) | Message::Pong(_) => {}
            _ => return Err("the browser closed or sent an unexpected control message".into()),
        }
    }
}

fn save_screenshot(shot: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let png = data_encoding::BASE64.decode(shot["png"].as_str().ok_or("the browser returned no PNG")?.as_bytes())?;
    // PNG signature and IHDR with the two big-endian dimensions.
    let Some([w0, w1, w2, w3, h0, h1, h2, h3, ..]) = png.strip_prefix(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR") else {
        return Err("the browser returned an invalid PNG".into());
    };
    let width = u32::from_be_bytes([*w0, *w1, *w2, *w3]);
    let height = u32::from_be_bytes([*h0, *h1, *h2, *h3]);
    if width == 0 || height == 0 {
        return Err("the browser returned an empty PNG".into());
    }
    let mut file = tempfile::Builder::new()
        .prefix("ponko-screenshot-")
        .suffix(".png")
        .tempfile()?;
    file.write_all(&png)?;
    let (_, path) = file.keep()?;
    Ok(json!({"path":path,"width":width,"height":height}))
}
