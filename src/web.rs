//! Web builds (wasm32 with the `window` feature): the game draws into a
//! canvas on the page, panics show in the browser console, and asset files
//! are fetched over HTTP before setup runs: the page's asset pack (named by
//! the canvas's `data-pack`, fetched with range requests), or the loose
//! files listed in `assets/index.json`.

pub(crate) mod ime;

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use winit::platform::web::WindowAttributesExtWebSys;
use winit::window::WindowAttributes;

/// The id of the `<canvas>` the game draws into, when the page has one.
/// Without it, the engine adds a canvas at the end of the page.
pub(crate) const CANVAS_ID: &str = "ponko";

/// The list of asset files to fetch, relative to the page: a JSON array of
/// names under `assets/`, such as `["crab.png", "fonts/Title.ttf"]`.
const INDEX: &str = "assets/index.json";

/// The value of `name` in the page's query string (`?name=value&...`):
/// `Some("")` when it's there without a value, `None` when it isn't there.
pub(crate) fn query_param(name: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    crate::query::value(&search, name)
}

/// Sends panic messages to the browser console (they're lost otherwise).
pub(crate) fn show_panics_in_console() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&info.to_string()));
    }));
}

/// Draw into `<canvas id="ponko">`, else a new canvas added to the page.
pub(crate) fn canvas_attributes(attributes: WindowAttributes) -> WindowAttributes {
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(CANVAS_ID))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    match canvas {
        Some(canvas) => attributes.with_canvas(Some(canvas)),
        None => attributes.with_append(true),
    }
}

/// The bytes asked for first from an asset pack: the header and the index
/// of most packs (an entry takes about 30 bytes and its name), and all of
/// a small pack.
const FIRST_RANGE: u64 = 64 * 1024;

/// The asset pack the page ships, from the canvas's `data-pack` attribute
/// (`<canvas id="ponko" data-pack="assets.pack">`, as `ponko package --web`
/// writes it), relative to the page.
fn page_pack() -> Option<String> {
    web_sys::window()?
        .document()?
        .get_element_by_id(CANVAS_ID)?
        .get_attribute("data-pack")
        .filter(|url| !url.is_empty())
}

/// Fetches the game's assets before setup, so that `load_texture` and
/// friends find them: the page's asset pack when the canvas names one,
/// else every file listed in `assets/index.json`, all at once. A page
/// with neither has no assets, which is fine for games that make their
/// textures in code.
pub(crate) async fn fetch_assets() {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(url) = page_pack() {
        match fetch_pack(&window, &url).await {
            Ok(pack) => {
                crate::assets::set_web_pack(pack);
                return;
            }
            Err(e) => log_error(&format!(
                "ponko: could not load the asset pack {url}: {e}; trying assets/index.json"
            )),
        }
    }
    let names: Vec<String> = match bytes(window.fetch_with_str(INDEX)).await {
        Ok(json) => serde_json::from_slice(&json).unwrap_or_else(|e| {
            log_error(&format!("ponko: {INDEX} isn't a JSON list of file names: {e}"));
            Vec::new()
        }),
        Err(_) => return,
    };
    // Start every request before waiting for any.
    let requests: Vec<_> = names
        .into_iter()
        .map(|name| {
            let request = window.fetch_with_str(&format!("assets/{name}"));
            (name, request)
        })
        .collect();
    for (name, request) in requests {
        match bytes(request).await {
            Ok(data) => crate::assets::insert_web_file(name, data),
            Err(e) => log_error(&format!("ponko: could not fetch assets/{name}: {e}")),
        }
    }
}

/// Fetches an asset pack with at most two range requests: its first
/// [`FIRST_RANGE`] bytes, whose header says how long it is (and which hold
/// all of a small pack), then the rest. The header is checked before the
/// rest is asked for. A server that ignores ranges sends the whole pack in
/// answer to the first.
async fn fetch_pack(window: &web_sys::Window, url: &str) -> Result<crate::assets::PackSource, String> {
    let (status, mut bytes) = ranged(window, url, 0, FIRST_RANGE - 1).await?;
    if status == 206 {
        let header = crate::assets::parse_pack_header(&bytes)?.ok_or("it is not an asset pack")?;
        let total = header.total_len();
        if (bytes.len() as u64) < total {
            let (status, rest) = ranged(window, url, bytes.len() as u64, total - 1).await?;
            if status == 206 {
                bytes.extend_from_slice(&rest);
            } else {
                bytes = rest;
            }
        }
    }
    crate::assets::PackSource::from_bytes(url, bytes).map_err(|e| e.message().to_string())
}

/// A `fetch` of bytes `from..=to` of `url`: the status (206 for the range,
/// 200 for the whole file) and the body.
async fn ranged(window: &web_sys::Window, url: &str, from: u64, to: u64) -> Result<(u16, Vec<u8>), String> {
    let headers = web_sys::Headers::new().map_err(describe)?;
    headers.set("Range", &format!("bytes={from}-{to}")).map_err(describe)?;
    let init = web_sys::RequestInit::new();
    init.set_headers(&headers);
    let response = response(window.fetch_with_str_and_init(url, &init)).await?;
    let status = response.status();
    Ok((status, body(response).await?))
}

/// Waits for a `fetch` and returns the body.
async fn bytes(request: js_sys::Promise) -> Result<Vec<u8>, String> {
    body(response(request).await?).await
}

/// Waits for a `fetch`'s response; an HTTP error status is an error.
async fn response(request: js_sys::Promise) -> Result<web_sys::Response, String> {
    let response: web_sys::Response = JsFuture::from(request)
        .await
        .map_err(describe)?
        .dyn_into()
        .map_err(describe)?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    Ok(response)
}

/// Waits for a response's whole body.
async fn body(response: web_sys::Response) -> Result<Vec<u8>, String> {
    let body = JsFuture::from(response.array_buffer().map_err(describe)?)
        .await
        .map_err(describe)?;
    Ok(js_sys::Uint8Array::new(&body).to_vec())
}

fn describe(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

pub(crate) fn log_error(message: &str) {
    web_sys::console::error_1(&JsValue::from_str(message));
}
