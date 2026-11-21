//! The editor in a web build: where Save puts files, and Download.
//!
//! A browser has no `assets/` folder to write to, so a web build with the
//! editor keeps the files it saves (scenes, prefabs) in the page's
//! `localStorage`: one entry per file, its text under the key
//! `ponko-assets:<page path>:<asset name>`, so two games on one site keep
//! theirs apart. A saved file is read from memory at once, and before setup
//! on the next visit the entries are put back over the fetched files, so
//! the editor (and the game) find the saved version. Browsers keep a few
//! megabytes per site this way; **Download** hands the person the file
//! itself, to keep it or put it in the project's `assets/`.
//!
//! The parts that don't need a browser (the keys, what is kept and put
//! back) are written against [`Store`], so tests run them on a map.

// Natively only the tests use the store's logic.
#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

/// The start of every key this module keeps in `localStorage`.
pub(crate) const PREFIX: &str = "ponko-assets:";

/// Text entries by key: the page's `localStorage`, or a map in tests.
pub(crate) trait Store {
    fn get(&self, key: &str) -> Option<String>;
    /// Keeps `value` under `key`; an error is the browser's (out of space).
    fn set(&mut self, key: &str, value: &str) -> Result<(), String>;
    fn keys(&self) -> Vec<String>;
}

#[cfg(test)]
impl Store for std::collections::BTreeMap<String, String> {
    fn get(&self, key: &str) -> Option<String> {
        std::collections::BTreeMap::get(self, key).cloned()
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn keys(&self) -> Vec<String> {
        std::collections::BTreeMap::keys(self).cloned().collect()
    }
}

/// The key asset `name` of the page at `page` (its path) is kept under.
pub(crate) fn key(page: &str, name: &str) -> String {
    format!("{PREFIX}{page}:{name}")
}

/// The asset name `key` keeps for the page at `page`, if it is one of its.
pub(crate) fn name_of<'a>(page: &str, key: &'a str) -> Option<&'a str> {
    let name = key.strip_prefix(PREFIX)?.strip_prefix(page)?.strip_prefix(':')?;
    (!name.is_empty()).then_some(name)
}

/// Keeps asset `name` (`bytes`) for the page at `page` in `store`.
///
/// # Errors
///
/// For bytes that aren't UTF-8 text (the editor saves only JSON), and
/// when the browser has no room left.
pub(crate) fn keep_in(store: &mut impl Store, page: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| format!("can't save \"{name}\" in the browser: it keeps text files only"))?;
    store.set(&key(page, name), text).map_err(|e| {
        format!("can't save \"{name}\": the browser's storage refused it ({e}); File > Download gives the file instead")
    })
}

/// The assets `store` keeps for the page at `page`, by name, sorted.
pub(crate) fn kept_in(store: &impl Store, page: &str) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = store
        .keys()
        .iter()
        .filter_map(|k| {
            let name = name_of(page, k)?;
            Some((name.to_string(), store.get(k)?.into_bytes()))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The name a downloaded asset `name` gets: its file name, without folders.
pub(crate) fn download_name(name: &str) -> &str {
    name.rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("scene.json")
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::{JsCast, JsValue};

    /// The page's `localStorage`.
    pub(super) struct Local(web_sys::Storage);

    impl super::Store for Local {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get_item(key).ok().flatten()
        }
        fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
            self.0.set_item(key, value).map_err(describe)
        }
        fn keys(&self) -> Vec<String> {
            let n = self.0.length().unwrap_or(0);
            (0..n).filter_map(|i| self.0.key(i).ok().flatten()).collect()
        }
    }

    /// The page's `localStorage` and the page's path.
    pub(super) fn local() -> Result<(Local, String), String> {
        let window = web_sys::window().ok_or("no browser window (saves work on the page's thread only)")?;
        let page = window.location().pathname().map_err(describe)?;
        let storage = window
            .local_storage()
            .map_err(describe)?
            .ok_or("the browser has no localStorage for this page")?;
        Ok((Local(storage), page))
    }

    /// Hands `bytes` to the person as a download named `file`.
    pub(super) fn download(file: &str, bytes: &[u8]) -> Result<(), String> {
        let window = web_sys::window().ok_or("no browser window")?;
        let document = window.document().ok_or("no document")?;
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/json");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(describe)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(describe)?;
        let link: web_sys::HtmlAnchorElement = document
            .create_element("a")
            .map_err(describe)?
            .dyn_into()
            .map_err(|_| "not a link")?;
        link.set_href(&url);
        link.set_download(file);
        let body = document.body().ok_or("no page body")?;
        body.append_child(&link).map_err(describe)?;
        link.click();
        let _ = body.remove_child(&link);
        // Let go of the file once the browser has started the download.
        let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000);
        Ok(())
    }

    fn describe(value: JsValue) -> String {
        value
            .dyn_ref::<js_sys::Error>()
            .map(|e| String::from(e.message()))
            .or_else(|| value.as_string())
            .unwrap_or_else(|| format!("{value:?}"))
    }
}

/// Keeps saved asset `name` in the page's `localStorage` (see the module
/// docs).
#[cfg(target_arch = "wasm32")]
pub(crate) fn keep(name: &str, bytes: &[u8]) -> Result<(), String> {
    let (mut store, page) = browser::local().map_err(|e| format!("can't save \"{name}\": {e}"))?;
    keep_in(&mut store, &page, name, bytes)
}

/// Puts the assets saved on this page back, over the fetched ones. Runs
/// before setup, after the assets were fetched.
#[cfg(target_arch = "wasm32")]
pub(crate) fn restore() {
    let Ok((store, page)) = browser::local() else { return };
    for (name, bytes) in kept_in(&store, &page) {
        crate::assets::insert_web_file(name, bytes);
    }
}

/// Hands asset `name` (`bytes`) to the person as a download.
#[cfg(target_arch = "wasm32")]
pub(crate) fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    browser::download(download_name(name), bytes).map_err(|e| format!("can't download \"{name}\": {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn a_saved_scene_comes_back_for_its_page_only() {
        let mut store = BTreeMap::new();
        let scene = br#"{"format":"ponko-world","entities":[]}"#;
        keep_in(&mut store, "/game/", "scenes/level.json", scene).unwrap();
        keep_in(&mut store, "/other/", "scenes/level.json", b"{}").unwrap();
        // Something else the site keeps.
        store.insert("settings".into(), "{}".into());

        let kept = kept_in(&store, "/game/");
        assert_eq!(kept, vec![("scenes/level.json".to_string(), scene.to_vec())]);
        assert_eq!(kept_in(&store, "/other/").len(), 1);
        assert!(kept_in(&store, "/game").is_empty(), "the page's path matches whole");
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn saving_again_replaces_the_kept_file() {
        let mut store = BTreeMap::new();
        keep_in(&mut store, "/", "a.json", b"1").unwrap();
        keep_in(&mut store, "/", "a.json", b"2").unwrap();
        assert_eq!(kept_in(&store, "/"), vec![("a.json".to_string(), b"2".to_vec())]);
    }

    #[test]
    fn only_text_is_kept_and_a_full_store_says_to_download() {
        let mut store = BTreeMap::new();
        let err = keep_in(&mut store, "/", "x.json", &[0xff, 0xfe]).unwrap_err();
        assert!(err.contains("text files only"), "{err}");
        assert!(store.is_empty());

        struct Full;
        impl Store for Full {
            fn get(&self, _: &str) -> Option<String> {
                None
            }
            fn set(&mut self, _: &str, _: &str) -> Result<(), String> {
                Err("QuotaExceededError".into())
            }
            fn keys(&self) -> Vec<String> {
                Vec::new()
            }
        }
        let err = keep_in(&mut Full, "/", "scenes/big.json", b"{}").unwrap_err();
        assert!(err.contains("QuotaExceededError") && err.contains("Download"), "{err}");
    }

    #[test]
    fn keys_name_the_page_and_the_asset() {
        assert_eq!(key("/play/", "scenes/a.json"), "ponko-assets:/play/:scenes/a.json");
        assert_eq!(
            name_of("/play/", &key("/play/", "scenes/a.json")),
            Some("scenes/a.json")
        );
        assert_eq!(name_of("/play/", "ponko-assets:/play/:"), None);
        assert_eq!(name_of("/play/", "elsewhere"), None);
        assert_eq!(download_name("scenes/a.json"), "a.json");
        assert_eq!(download_name("a.json"), "a.json");
    }
}
