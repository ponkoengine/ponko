//! Which other crates' types Ponko's stable API may name (STABILITY.md,
//! "Re-exported crates"). `glam` and `serde_json` are kept and pinned;
//! `steamworks` and `taffy` are reachable only through `unstable_` names and
//! the few unstable items listed below, so a new version of either is not a
//! breaking Ponko release.
//!
//! The test reads the API listing in `api/ponko/docs-rs.txt` (every feature
//! docs.rs builds, `steam` among them), which `scripts/api-snapshots.py check`
//! keeps equal to the code. It needs no Steam installation.

const DOCS_RS_API: &str = include_str!("../api/ponko/docs-rs.txt");

/// Crates whose types may appear only on items with `unstable_` in their name
/// (`unstable_taffy`, `UiLayout::into_unstable_style`), and the other items
/// that name them (an item's path, or an `impl` line as listed), with the
/// reason.
const WRAPPED: &[(&str, &[&str])] = &[
    (
        "steamworks::",
        // The `From` conversions between Ponko's Steam ids and the crate's,
        // for calls through `SteamClient::unstable_steamworks`; STABILITY.md
        // lists them under "Unstable".
        &[
            "impl core::convert::From<ponko::net::SteamLobbyId> for steamworks::matchmaking::LobbyId",
            "steamworks::matchmaking::LobbyId::from",
            "impl core::convert::From<steamworks::matchmaking::LobbyId> for ponko::net::SteamLobbyId",
            "ponko::net::SteamLobbyId::from",
            "impl core::convert::From<ponko::net::SteamUserId> for steamworks::SteamId",
            "steamworks::SteamId::from",
            "impl core::convert::From<steamworks::SteamId> for ponko::net::SteamUserId",
            "ponko::net::SteamUserId::from",
        ],
    ),
    (
        "taffy::",
        // The deprecated old names of `UiLayout::unstable_*`, removed in the
        // next minor; STABILITY.md lists them under "Unstable".
        &[
            "ponko::prelude::UiLayout::custom",
            "ponko::prelude::UiLayout::into_style",
            "ponko::prelude::UiLayout::style",
            "ponko::prelude::UiLayout::style_mut",
        ],
    ),
];

/// The item a listing line declares (`pub fn ponko::net::Net::steam_lobby(...)`
/// declares `ponko::net::Net::steam_lobby`), or `None` for an `impl` line.
fn item(line: &str) -> Option<&str> {
    let mut rest = line;
    loop {
        let before = rest;
        for keyword in [
            "pub ",
            "unsafe ",
            "const ",
            "async ",
            "extern \"C\" ",
            "fn ",
            "struct ",
            "enum ",
            "union ",
            "use ",
            "type ",
            "trait ",
            "mod ",
            "static ",
            "macro ",
        ] {
            rest = rest.strip_prefix(keyword).unwrap_or(rest);
        }
        if rest == before {
            break;
        }
    }
    if rest.starts_with("impl") || rest.starts_with("#[") {
        return None;
    }
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(rest.len());
    Some(rest[..end].trim_end_matches(':'))
}

/// The lines of `listing` that name a wrapped crate's types on a stable item.
fn leaks(listing: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in listing.lines() {
        for (krate, allowed) in WRAPPED {
            if !line.contains(krate) {
                continue;
            }
            let ok = allowed.contains(&line)
                || item(line)
                    .is_some_and(|name| name.split("::").any(|s| s.contains("unstable_")) || allowed.contains(&name));
            if !ok {
                found.push(line.to_owned());
            }
        }
    }
    found
}

#[test]
fn stable_api_names_no_steamworks_or_taffy_types() {
    let found = leaks(DOCS_RS_API);
    assert!(
        found.is_empty(),
        "stable API names a wrapped crate's types; wrap them in Ponko types or give the item an \
         `unstable_` name (STABILITY.md, \"Re-exported crates\"):\n{}",
        found.join("\n")
    );
}

#[test]
fn raw_crates_are_reexported_only_under_unstable_names() {
    let uses: Vec<_> = DOCS_RS_API.lines().filter(|l| l.starts_with("pub use ")).collect();
    assert!(uses.contains(&"pub use ponko::net::unstable_steamworks"), "{uses:?}");
    assert!(uses.contains(&"pub use ponko::ui::unstable_taffy"), "{uses:?}");
    assert!(
        !uses.contains(&"pub use ponko::net::steam"),
        "0.2.0 moved `ponko::net::steam` to `ponko::net::unstable_steamworks`"
    );
}

#[test]
fn the_check_catches_the_old_steam_api() {
    // Negative control: lines from the 0.1 listing, before Steam was wrapped,
    // and a conversion that is not one of the listed unstable ones.
    let old = "pub fn ponko::net::Net::steam_lobby(&self) -> core::option::Option<steamworks::matchmaking::LobbyId>\n\
               pub fn ponko::net::SteamLobby::members(&self) -> alloc::vec::Vec<steamworks::SteamId>\n\
               impl core::convert::From<steamworks::SteamId> for ponko::net::PeerId\n\
               impl core::convert::From<steamworks::SteamId> for ponko::net::SteamUserId\n\
               pub fn ponko::prelude::UiLayout::custom(taffy::style::Style) -> Self\n\
               pub fn ponko::net::SteamClient::unstable_steamworks(&self) -> &steamworks::Client";
    assert_eq!(leaks(old).len(), 3, "{:?}", leaks(old));
}
