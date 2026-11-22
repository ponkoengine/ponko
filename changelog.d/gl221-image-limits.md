### Added
- `ImageLimits` (longest side in pixels, most decoded bytes) is the one setting every image decoder reads:
  PNG, JPEG and the other `image-*` formats, KTX2, DDS, the pictures of glTF models, tilesets and sprite
  sheets, and the frames of a video. The defaults stay (16 384 pixels, 512 MiB); `Game::image_limits` or
  `Assets::set_image_limits` change them for the process, and `Assets::image_limits` reads them. A picture
  past them is refused with `ErrorKind::Limit` before its pixels are allocated, and the message names the
  limits in force. Lower them for pictures from players or the network, raise them for large source art.
- `Graphics::try_decode_texture(bytes, name, limits)` decodes one untrusted picture under limits of its own,
  without changing the game's.
- C: `ponko_game_image_limits`, `ponko_ctx_set_image_limits`, `ponko_ctx_image_limits` and
  `ponko_ctx_try_decode_texture`; C++: `game::image_limits`, `ctx::set_image_limits`, `ctx::image_limits` and
  `ctx::try_decode_texture`. The security chapter of the guide says when to change them.
