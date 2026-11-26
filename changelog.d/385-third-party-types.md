### Breaking changes
- The Steam API takes and returns Ponko's own types: `ponko::net::SteamClient`,
  `SteamLobbyId` and `SteamUserId` replace `steamworks`' `Client`, `LobbyId`
  and `SteamId` in `Net::steam_host`, `steam_join`, `steam_lobby`,
  `NetMessages::steam_host`, `steam_join`, `steam_peer`, `SteamLobby::create`,
  `join`, `id`, `members`, `steam_lobby_from_args` and
  `steam_wait_for_invite`. A new `steamworks` release is no longer a breaking
  Ponko release. Feature `steam`; the C API is unchanged.
  Instead: start Steam with `SteamClient::init_app` (or `SteamClient::init`)
  and pass it where the `steamworks::Client` went. Lobby and user ids convert
  with `from_raw` and `raw`, print and parse as their 64-bit number.
  Before:
  ```rust
  let client = steamworks::Client::init_app(480).expect("is Steam running?");
  let net = match ponko::net::steam_lobby_from_args() {
      Some(lobby) => Net::steam_join(&client, lobby),
      None => Net::steam_host(&client, 2),
  };
  let lobby = steamworks::LobbyId::from_raw(id);
  ```
  After:
  ```rust
  use ponko::net::Net;
  # let id: u64 = todo!();
  let client = ponko::net::SteamClient::init_app(480)?;
  let net = match ponko::net::steam_lobby_from_args() {
      Some(lobby) => Net::steam_join(&client, lobby),
      None => Net::steam_host(&client, 2),
  };
  let lobby = ponko::net::SteamLobbyId::from_raw(id);
  ```
- `ponko::net::steam`, the re-exported `steamworks` crate, is `ponko::net::unstable_steamworks` now.
  Like `ui::unstable_taffy`, it is outside the versions: its `steamworks`
  version may change in any release.
  Instead: use Ponko's Steam types; for the parts of Steam Ponko doesn't wrap
  (achievements, stats, rich presence), take the client underneath with
  `SteamClient::unstable_steamworks`, and pass ids across as `u64`.
  Before:
  ```rust
  let client = ponko::net::steam::Client::init_app(480).unwrap();
  let _ = client.user_stats().achievement("WIN_ONE").set();
  ```
  After:
  ```rust
  let client = ponko::net::SteamClient::init_app(480)?;
  let _ = client.unstable_steamworks().user_stats().achievement("WIN_ONE").set();
  ```

### Changed
- STABILITY.md, "Re-exported crates", has the final rule for 1.0: `glam`
  (`ponko::math`, the prelude) and `serde_json` (`ponko::reflect::json`,
  `JsonValue`, `JsonMap`, `json!`, and the same names in `ponko_mcp`) stay
  re-exported and pinned, so from 1.0 on an incompatible version of either
  comes only with a Ponko major release; `steamworks` is wrapped and `taffy`
  stays `ui::unstable_taffy`. A test checks the API listing for `steamworks`
  and `taffy` types outside `unstable_` items.
