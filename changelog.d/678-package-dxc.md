### Changed
- Windows game packages bundle Microsoft's DirectX Shader Compiler
  (`dxcompiler.dll`) beside the executable and include its licence in
  `THIRD-PARTY-NOTICES.txt`. The CLI caches the pinned release ZIP and
  verifies its SHA-256. Set `windows-dxc = false` under
  `[package.metadata.ponko]` to leave the DLL out and use FXC when DXC
  is unavailable. Development builds still use wgpu's default compiler
  selection.
