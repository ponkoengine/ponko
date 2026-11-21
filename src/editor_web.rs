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
//! File > Saved in this browser lists the entries, and reverts one: its
//! entry is removed, and the build's file (kept aside when the save
//! replaced it) is read again at once; a file the build doesn't have is
//! deleted. After a reload the shipped file is back, as nothing replaces
//! it any more. [`Assets::browser_saves`](crate::assets::Assets::browser_saves)
//! and [`Assets::revert_browser_save`](crate::assets::Assets::revert_browser_save)
//! do the same from code.
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
    fn remove(&mut self, key: &str) -> Result<(), String>;
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
    fn remove(&mut self, key: &str) -> Result<(), String> {
        std::collections::BTreeMap::remove(self, key);
        Ok(())
    }
    fn keys(&self) -> Vec<String> {
        std::collections::BTreeMap::keys(self).cloned().collect()
    }
}

/// The build's own files that saves replaced, by asset name: what a revert
/// puts back.
pub(crate) type Shipped = std::collections::BTreeMap<String, Vec<u8>>;

/// A file saved in the browser, as File > Saved in this browser lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Saved {
    /// Its asset name.
    pub name: String,
    /// The bytes kept.
    pub bytes: usize,
    /// Whether the build has its own file (a revert puts it back), or the
    /// file exists only in the browser (a revert deletes it).
    pub shipped: bool,
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

/// Notes the build's file `current` of asset `name` (what the page reads
/// now) in `shipped` before a save replaces it, unless the page reads a
/// file saved earlier (an entry in `store`) or it is noted already.
pub(crate) fn remember_shipped(
    shipped: &mut Shipped,
    store: &impl Store,
    page: &str,
    name: &str,
    current: impl FnOnce() -> Option<Vec<u8>>,
) {
    if shipped.contains_key(name) || store.get(&key(page, name)).is_some() {
        return;
    }
    if let Some(bytes) = current() {
        shipped.insert(name.to_string(), bytes);
    }
}

/// The files `store` keeps for the page at `page`, sorted by name;
/// `shipped` says which ones the build has too.
pub(crate) fn saved_in(store: &impl Store, page: &str, shipped: &Shipped) -> Vec<Saved> {
    kept_in(store, page)
        .into_iter()
        .map(|(name, bytes)| Saved {
            shipped: shipped.contains_key(&name),
            bytes: bytes.len(),
            name,
        })
        .collect()
}

/// Removes the entry of asset `name` of the page at `page` from `store`.
/// Returns whether there was one.
///
/// # Errors
///
/// When the browser refuses (it doesn't, for a removal, in practice).
pub(crate) fn forget_in(store: &mut impl Store, page: &str, name: &str) -> Result<bool, String> {
    let key = key(page, name);
    if store.get(&key).is_none() {
        return Ok(false);
    }
    store
        .remove(&key)
        .map(|()| true)
        .map_err(|e| format!("can't revert \"{name}\": the browser's storage refused ({e})"))
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

    thread_local! {
        /// The build's files that saves replaced (see `remember_shipped`).
        pub(super) static SHIPPED: std::cell::RefCell<super::Shipped> = Default::default();
    }

    /// The page's `localStorage`.
    pub(super) struct Local(web_sys::Storage);

    impl super::Store for Local {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get_item(key).ok().flatten()
        }
        fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
            self.0.set_item(key, value).map_err(describe)
        }
        fn remove(&mut self, key: &str) -> Result<(), String> {
            self.0.remove_item(key).map_err(describe)
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
    browser::SHIPPED.with(|shipped| {
        let current = || crate::assets::read_web_asset(name);
        remember_shipped(&mut shipped.borrow_mut(), &store, &page, name, current);
    });
    keep_in(&mut store, &page, name, bytes)
}

/// Puts the assets saved on this page back, over the fetched ones (kept
/// aside, to revert to). Runs before setup, after the assets were fetched.
#[cfg(target_arch = "wasm32")]
pub(crate) fn restore() {
    let Ok((store, page)) = browser::local() else { return };
    for (name, bytes) in kept_in(&store, &page) {
        if let Some(fetched) = crate::assets::read_web_asset(&name) {
            browser::SHIPPED.with(|shipped| {
                shipped.borrow_mut().entry(name.clone()).or_insert(fetched);
            });
        }
        crate::assets::insert_web_file(name, bytes);
    }
}

/// The files saved in this browser for this page, sorted by name.
#[cfg(target_arch = "wasm32")]
pub(crate) fn saved() -> Vec<Saved> {
    let Ok((store, page)) = browser::local() else {
        return Vec::new();
    };
    browser::SHIPPED.with(|shipped| saved_in(&store, &page, &shipped.borrow()))
}

/// Reverts asset `name` to the build's file: removes its entry, and puts
/// the build's file back in memory (or drops the file, if the build has
/// none). Returns whether it was saved in the browser.
#[cfg(target_arch = "wasm32")]
pub(crate) fn forget(name: &str) -> Result<bool, String> {
    let (mut store, page) = browser::local().map_err(|e| format!("can't revert \"{name}\": {e}"))?;
    if !forget_in(&mut store, &page, name)? {
        return Ok(false);
    }
    match browser::SHIPPED.with(|shipped| shipped.borrow().get(name).cloned()) {
        Some(bytes) => crate::assets::insert_web_file(name.to_string(), bytes),
        None => crate::assets::remove_web_file(name),
    }
    Ok(true)
}

/// Native tests of the web editor: the browser's storage is a map on the
/// test's thread (page `/`), and saving doesn't use it (files go to disk).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod test_browser {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        pub(crate) static STORE: RefCell<BTreeMap<String, String>> = RefCell::default();
        pub(crate) static SHIPPED: RefCell<super::Shipped> = RefCell::default();
    }

    /// Keeps `text` as asset `name` in the browser; `shipped` is the
    /// build's own file it replaced, if any.
    pub(crate) fn keep(name: &str, text: &str, shipped: Option<&[u8]>) {
        STORE.with(|s| super::keep_in(&mut *s.borrow_mut(), "/", name, text.as_bytes()).unwrap());
        if let Some(bytes) = shipped {
            SHIPPED.with(|s| s.borrow_mut().insert(name.to_string(), bytes.to_vec()));
        }
    }
}

/// The files saved in the browser: in native tests, `test_browser`'s.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn saved() -> Vec<Saved> {
    test_browser::STORE.with(|s| test_browser::SHIPPED.with(|sh| saved_in(&*s.borrow(), "/", &sh.borrow())))
}

/// Reverts asset `name`: in native tests, in `test_browser`'s storage.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn forget(name: &str) -> Result<bool, String> {
    test_browser::STORE.with(|s| forget_in(&mut *s.borrow_mut(), "/", name))
}

/// Desktop builds keep nothing in a browser.
#[cfg(not(any(test, target_arch = "wasm32")))]
pub(crate) fn saved() -> Vec<Saved> {
    Vec::new()
}

/// Desktop builds keep nothing in a browser: nothing to revert.
#[cfg(not(any(test, target_arch = "wasm32")))]
pub(crate) fn forget(_name: &str) -> Result<bool, String> {
    Ok(false)
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
            fn remove(&mut self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn keys(&self) -> Vec<String> {
                Vec::new()
            }
        }
        let err = keep_in(&mut Full, "/", "scenes/big.json", b"{}").unwrap_err();
        assert!(err.contains("QuotaExceededError") && err.contains("Download"), "{err}");
    }

    #[test]
    fn a_revert_removes_the_entry_and_the_build_s_file_is_noted_once() {
        let mut store = BTreeMap::new();
        let mut shipped = Shipped::new();
        let build = br#"{"format":"ponko-world","entities":[]}"#.to_vec();
        // The first save over a file the build has notes the build's file.
        remember_shipped(&mut shipped, &store, "/", "levels/one.json", || Some(build.clone()));
        keep_in(&mut store, "/", "levels/one.json", b"{\"saved\":1}").unwrap();
        // A second save: the page reads the first save now, not the build's.
        remember_shipped(&mut shipped, &store, "/", "levels/one.json", || {
            Some(b"{\"saved\":1}".to_vec())
        });
        keep_in(&mut store, "/", "levels/one.json", b"{\"saved\":2}").unwrap();
        assert_eq!(
            shipped.get("levels/one.json"),
            Some(&build),
            "the build's file, not a save"
        );
        // A file the build doesn't have.
        remember_shipped(&mut shipped, &store, "/", "levels/new.json", || None);
        keep_in(&mut store, "/", "levels/new.json", b"{}").unwrap();
        keep_in(&mut store, "/other/", "levels/one.json", b"{}").unwrap();

        let saved = saved_in(&store, "/", &shipped);
        let listed: Vec<(&str, usize, bool)> = saved.iter().map(|s| (s.name.as_str(), s.bytes, s.shipped)).collect();
        assert_eq!(listed, [("levels/new.json", 2, false), ("levels/one.json", 11, true)]);

        assert_eq!(forget_in(&mut store, "/", "levels/one.json"), Ok(true));
        assert_eq!(forget_in(&mut store, "/", "levels/one.json"), Ok(false), "gone already");
        assert_eq!(kept_in(&store, "/").len(), 1, "the other entry stays");
        assert_eq!(kept_in(&store, "/other/").len(), 1, "another page's stays");
        assert_eq!(forget_in(&mut store, "/", "levels/new.json"), Ok(true));
        assert!(
            kept_in(&store, "/").is_empty(),
            "nothing is put back over the build's files any more"
        );
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
