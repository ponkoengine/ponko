//! Browser transport for the same control requests the native endpoint handles.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MessageEvent, Url, WebSocket};

use crate::ctx::Ctx;

const MAX_REQUEST: usize = 4 * 1024 * 1024;
// Leave room for the JSON envelope under the bridge's 32 MiB message limit.
const MAX_PNG_BASE64: usize = 31 * 1024 * 1024;

/// One tab has one bridge, with at most one request outstanding. The JS
/// callback queues it; only the normal frame loop has access to the game.
pub(crate) struct BrowserControl {
    socket: WebSocket,
    pending: Rc<RefCell<Option<String>>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

impl BrowserControl {
    pub(crate) fn from_page() -> Option<Self> {
        let location = web_sys::window()?.location();
        if !matches!(location.hostname().ok()?.as_str(), "127.0.0.1" | "localhost" | "[::1]") {
            return None;
        }
        let hash = location.hash().ok()?;
        let address = hash.strip_prefix("#ponko-control=")?;
        let url = Url::new(address).ok()?;
        let path = url.pathname();
        let token = path.strip_prefix('/')?;
        if url.protocol() != "ws:"
            || url.hostname() != "127.0.0.1"
            || url.port().is_empty()
            || !url.username().is_empty()
            || !url.password().is_empty()
            || !url.search().is_empty()
            || !url.hash().is_empty()
            || token.len() != 32
            || !token.bytes().all(|b| b.is_ascii_hexdigit())
        {
            crate::web::log_error("ponko: use the local URL printed by ponko-mcp --web-bridge");
            return None;
        }
        let socket = match WebSocket::new(address) {
            Ok(socket) => socket,
            Err(_) => {
                crate::web::log_error("ponko: could not open the browser control socket");
                return None;
            }
        };
        let pending = Rc::new(RefCell::new(None::<String>));
        let inbox = pending.clone();
        let on_message = Closure::new(move |message: MessageEvent| {
            let mut request = inbox.borrow_mut();
            if request.is_none()
                && let Some(text) = message.data().as_string()
                && text.len() <= MAX_REQUEST
            {
                *request = Some(text);
            }
        });
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Some(Self {
            socket,
            pending,
            _on_message: on_message,
        })
    }

    /// Dispatch between frames. A screenshot is answered after this frame
    /// is drawn, before returning the canvas to the browser compositor.
    pub(crate) fn poll(&self, ctx: &mut Ctx) -> Option<(Value, u32)> {
        let line = self.pending.borrow_mut().take()?;
        let req: Value = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                self.reply(&Value::Null, Err(format!("invalid JSON: {e}")));
                return None;
            }
        };
        let id = &req["id"];
        let params = &req["params"];
        if req["method"] == "screenshot" {
            match params.get("max_size").filter(|v| !v.is_null()) {
                None => return Some((id.clone(), 0)),
                Some(value) => {
                    if let Some(max) = value.as_u64().and_then(|n| u32::try_from(n).ok()) {
                        return Some((id.clone(), max));
                    }
                    self.reply(id, Err("`max_size` must be a non-negative 32-bit integer".into()));
                    return None;
                }
            }
        }
        self.reply(id, dispatch(ctx, req["method"].as_str(), params));
        None
    }

    pub(crate) fn screenshot(&self, id: &Value, max_size: u32, canvas: Option<HtmlCanvasElement>) {
        let result = canvas
            .ok_or_else(|| "the game has no canvas to capture".to_owned())
            .and_then(|canvas| capture(canvas, max_size));
        self.reply(id, result);
    }

    fn reply(&self, id: &Value, response: Result<Value, String>) {
        let reply = match response {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": error }),
        };
        let _ = self.socket.send_with_str(&reply.to_string());
    }
}

impl Drop for BrowserControl {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        let _ = self.socket.close();
    }
}

fn available(method: &str) -> bool {
    matches!(
        method,
        "info"
            | "methods"
            | "capabilities"
            | "world.summary"
            | "entities.list"
            | "entities.stats"
            | "entity.get"
            | "resources.list"
            | "resource.get"
            | "systems.timings"
            | "ecs.diagnostics"
            | "perf.get"
            | "shaders.status"
            | "camera.get"
            | "sim.pause"
            | "sim.resume"
            | "sim.step"
            | "sim.speed"
            | "screenshot"
    )
}

fn dispatch(ctx: &mut Ctx, method: Option<&str>, params: &Value) -> Result<Value, String> {
    let method = method.ok_or("missing `method`")?;
    if !available(method) {
        return Err(format!("`{method}` is unavailable over browser control"));
    }
    let mut result = super::handle(ctx, method, params)?;
    if method == "methods"
        && let Some(methods) = result.as_array_mut()
    {
        methods.retain(|m| m["name"].as_str().is_some_and(available));
    }
    if method == "capabilities"
        && let Some(methods) = result["methods"].as_array_mut()
    {
        for m in methods {
            if !m["name"].as_str().is_some_and(available) {
                m["available"] = json!(false);
                m["reason"] = json!("unavailable over browser control");
            } else if m["name"] == "screenshot" {
                m["about"] = json!(
                    "capture the next canvas frame as a local PNG; params: max_size? (0 = full size). Returns path, width, height"
                );
            }
        }
        result["subsystems"]["control"]["transport"] = json!("web");
    }
    Ok(result)
}

fn capture(canvas: HtmlCanvasElement, max_size: u32) -> Result<Value, String> {
    let (width, height) = (canvas.width(), canvas.height());
    if width == 0 || height == 0 {
        return Err("the game's canvas has no pixels to capture".into());
    }
    let largest = width.max(height);
    let canvas = if max_size == 0 || max_size >= largest {
        canvas
    } else {
        let scale = |size: u32| ((u64::from(size) * u64::from(max_size)) / u64::from(largest)).max(1) as u32;
        let (width, height) = (scale(width), scale(height));
        let resized = canvas
            .owner_document()
            .ok_or("the canvas is not in a document")?
            .create_element("canvas")
            .map_err(|_| "could not create a screenshot canvas")?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| "could not create a screenshot canvas")?;
        resized.set_width(width);
        resized.set_height(height);
        let ctx = resized
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
            .ok_or("could not open the screenshot canvas")?;
        ctx.draw_image_with_html_canvas_element_and_dw_and_dh(&canvas, 0.0, 0.0, width.into(), height.into())
            .map_err(|_| "could not copy the game's canvas")?;
        resized
    };
    let url = canvas
        .to_data_url()
        .map_err(|_| "the browser could not read pixels from the canvas")?;
    let png = url
        .strip_prefix("data:image/png;base64,")
        .ok_or("the browser could not encode a PNG")?;
    if png.len() > MAX_PNG_BASE64 {
        return Err("the screenshot exceeds the bridge's message limit; use a smaller max_size".into());
    }
    Ok(json!({ "png": png }))
}
