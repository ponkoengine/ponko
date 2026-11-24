//! Native control methods through the authenticated loopback endpoint.
mod common;

use common::run_game;
use ponko_ffi::{PonkoStatus::*, *};
use serde_json::{Value, json};
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn native_control_method_receives_parameters_and_can_reply_or_reject() {
    unsafe extern "C" fn callback(
        ctx: *mut PonkoCtx,
        params: *const c_char,
        reply: *mut PonkoControlReply,
        _: *mut c_void,
    ) {
        // SAFETY: the game lends all three pointers until the callback returns.
        unsafe {
            let Ok(params) = serde_json::from_slice::<Value>(CStr::from_ptr(params).to_bytes()) else {
                return;
            };
            if let Some(value) = params["value"].as_i64() {
                let answer = CString::new(
                    json!({ "value": value + 1, "ctx_live": !ponko_ctx_world(ctx).is_null() }).to_string(),
                )
                .unwrap();
                let _ = ponko_control_reply_json(reply, answer.as_ptr());
            } else {
                let _ = ponko_control_reply_error(reply, c"value must be an integer".as_ptr());
            }
        }
    }

    let (tx, rx) = mpsc::channel();
    let game = thread::spawn(move || unsafe {
        let game = ponko_game_new(c"native-control-test".as_ptr(), 1, 1);
        assert_eq!(ponko_game_control_endpoint(game, true), PONKO_OK);
        assert_eq!(
            ponko_game_control_method(
                game,
                c"native.increment".as_ptr(),
                c"Adds one".as_ptr(),
                Some(callback),
                null_mut()
            ),
            PONKO_OK
        );
        let world = run_game(game, u64::MAX, |ctx| {
            let mut port = 0;
            assert_eq!(ponko_ctx_control_port(ctx, &mut port), PONKO_OK);
            tx.send(port).unwrap();
        });
        ponko_world_free(world);
    });

    let port = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let registry = match std::env::var_os("PONKO_INSTANCES_DIR") {
        Some(dir) => PathBuf::from(dir),
        None if cfg!(windows) => PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("ponko/instances"),
        None => PathBuf::from(std::env::var_os("HOME").unwrap()).join(".ponko/instances"),
    };
    let record = registry.join(format!("{}-{port}.json", std::process::id()));
    let record: Value = serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut call = |id, method, params| {
        writeln!(
            stream,
            "{}",
            json!({ "id": id, "token": record["token"], "method": method, "params": params })
        )
        .unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    let answer = call(1, "native.increment", json!({"value": 41}));
    let rejection = call(2, "native.increment", json!({"value": false}));
    let _ = call(3, "game.quit", json!({}));
    game.join().unwrap();
    assert_eq!(answer["result"], json!({"value":42, "ctx_live":true}));
    assert_eq!(rejection["error"], "value must be an integer");
}
