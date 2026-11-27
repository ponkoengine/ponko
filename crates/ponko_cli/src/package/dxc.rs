//! Package-time download of Microsoft's DX12 shader compiler.

use super::{archive, layout};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

const VERSION: &str = "v1.9.2609";
const ARCHIVE: &str = "dxc_2026_09_29.zip";
// GitHub's release asset digest, verified against the downloaded archive.
const SHA256: &str = "ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1";
const DLL: &str = "dxcompiler.dll";
const LICENSE: &str = "LICENSE-LLVM.txt";

pub(super) struct Dxc {
    pub(super) dll: Vec<u8>,
    pub(super) license: String,
}

impl Dxc {
    /// Put the compiler beside the executable and its license in the notices.
    pub(super) fn add_to_plan(self, mut plan: layout::Plan) -> Result<layout::Plan, String> {
        let path = plan.program.with_file_name(DLL);
        if let Some(conflict) = plan.entries.iter().find(|entry| {
            // The package might be staged on a case-sensitive host, but
            // Windows treats these paths as the same.
            let relative = entry.path.strip_prefix(&plan.folder).unwrap_or(&entry.path);
            relative
                .components()
                .next()
                .is_some_and(|part| part.as_os_str().eq_ignore_ascii_case(DLL))
        }) {
            return Err(format!(
                "{} conflicts with the packaged {DLL}; set windows-dxc = false to supply your own DLL",
                conflict.path.display()
            ));
        }
        let notice = plan.folder.join(layout::NOTICES);
        let text = plan.entries.iter_mut().find_map(|entry| match &mut entry.source {
            layout::Source::Text(text) if entry.path == notice => Some(text),
            _ => None,
        });
        let text = text.ok_or_else(|| format!("missing {} in the Windows package", notice.display()))?;
        let _ = writeln!(
            text,
            "\nMicrosoft DirectX Shader Compiler {VERSION} ({DLL})\n\
             https://github.com/microsoft/DirectXShaderCompiler/releases/tag/{VERSION}\n\
             {LICENSE}\n{}",
            self.license.trim_end()
        );
        plan.entries.push(layout::Entry {
            path,
            source: layout::Source::Bytes(self.dll),
            executable: false,
        });
        Ok(plan)
    }
}

pub(super) fn for_package(
    platform: &layout::Platform,
    enabled: bool,
    target_dir: &Path,
) -> Result<Option<Dxc>, String> {
    let url = format!("https://github.com/microsoft/DirectXShaderCompiler/releases/download/{VERSION}/{ARCHIVE}");
    let cache = target_dir.join("package-downloads").join(VERSION).join(ARCHIVE);
    load(platform, enabled, &cache, SHA256, || download(&url))
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    println!("ponko package: downloading DirectX Shader Compiler {VERSION}");
    let result = Command::new("curl")
        .arg("--disable") // Ignore settings from the user's curl config.
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--max-time",
            "120",
            "--max-filesize",
            "40000000",
            url,
        ])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("can't download DirectX Shader Compiler: {e}; install curl or set windows-dxc = false"))?;
    if !result.status.success() {
        return Err(format!("DirectX Shader Compiler download failed ({})", result.status));
    }
    Ok(result.stdout)
}

fn load(
    platform: &layout::Platform,
    enabled: bool,
    cache: &Path,
    sha256: &str,
    download: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Option<Dxc>, String> {
    if platform.os != layout::Os::Windows || !enabled {
        return Ok(None);
    }
    let arch = match platform.arch.as_str() {
        "x86_64" => "x64",
        "i686" => "x86",
        "aarch64" => "arm64",
        // DXC has no release binary for other Windows architectures.
        _ => return Ok(None),
    };
    let (archive, downloaded) = match fs::read(cache) {
        Ok(bytes) => (bytes, false),
        Err(e) if e.kind() == ErrorKind::NotFound => (download()?, true),
        Err(e) => return Err(format!("can't read {}: {e}", cache.display())),
    };
    let actual = format!("{:x}", Sha256::digest(&archive));
    if actual != sha256 {
        let subject = if downloaded {
            format!("the DirectX Shader Compiler {VERSION} download")
        } else {
            format!("{} (delete it to download again)", cache.display())
        };
        return Err(format!(
            "SHA-256 mismatch for {subject}: expected {sha256}, got {actual}"
        ));
    }
    let mut files = archive::unzip(&archive).map_err(|e| format!("DirectX Shader Compiler {VERSION}: {e}"))?;
    let mut take = |name: &str| {
        files
            .iter()
            .position(|file| file.name == name || file.name.replace('\\', "/") == name)
            .map(|index| files.swap_remove(index).bytes)
            .ok_or_else(|| format!("DirectX Shader Compiler {VERSION}: missing {name} in {ARCHIVE}"))
    };
    let dll = take(&format!("bin/{arch}/{DLL}"))?;
    let license = String::from_utf8(take(LICENSE)?).map_err(|e| format!("{LICENSE}: {e}"))?;
    if downloaded {
        store(cache, &archive).map_err(|e| format!("can't cache {}: {e}", cache.display()))?;
    }
    Ok(Some(Dxc { dll, license }))
}

fn store(cache: &Path, archive: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = cache.parent() {
        fs::create_dir_all(dir)?;
    }
    // Publish the verified archive with one rename so concurrent package
    // commands can only read the complete file.
    let temporary = cache.with_extension(format!("zip.{}.tmp", std::process::id()));
    let result = fs::write(&temporary, archive).and_then(|()| fs::rename(&temporary, cache));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cache(name: &str, test: impl FnOnce(&Path)) {
        struct Remove(std::path::PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let dir = Remove(std::env::temp_dir().join(format!("ponko-dxc-{name}-{}", std::process::id())));
        test(&dir.0.join("dxc.zip"));
    }

    fn fake_release() -> Vec<u8> {
        let files = [
            (r"bin\x64\dxcompiler.dll", "x64 compiler"),
            ("bin/arm64/dxcompiler.dll", "ARM64 compiler"),
            (r"bin\x86\dxcompiler.dll", "x86 compiler"),
            (r"bin\x64\dxil.dll", "not needed"),
            ("LICENSE-LLVM.txt", "Fake DXC license"),
        ];
        archive::apk_bytes(&files.map(|(name, data)| archive::ApkFile {
            name: name.into(),
            bytes: data.as_bytes().to_vec(),
        }))
        .unwrap()
    }

    #[test]
    fn verified_download_is_cached_and_reused_for_each_windows_architecture() {
        with_cache("reuse", |cache| {
            let release = fake_release();
            let hash = format!("{:x}", Sha256::digest(&release));
            let windows = |triple| layout::Platform::from_triple(triple).unwrap();

            let platform = windows("x86_64-pc-windows-gnu");
            let dxc = load(&platform, true, cache, &hash, || Ok(release.clone()))
                .unwrap()
                .unwrap();
            assert_eq!(dxc.dll, b"x64 compiler");
            assert_eq!(dxc.license, "Fake DXC license");
            assert_eq!(fs::read(cache).unwrap(), release);

            for (triple, expected) in [
                ("x86_64-pc-windows-msvc", "x64 compiler"),
                ("aarch64-pc-windows-msvc", "ARM64 compiler"),
                ("i686-pc-windows-msvc", "x86 compiler"),
            ] {
                let dxc = load(&windows(triple), true, cache, &hash, || panic!("downloaded twice"))
                    .unwrap()
                    .unwrap();
                assert_eq!(dxc.dll, expected.as_bytes());
            }
        });
    }

    #[test]
    fn bad_checksums_refuse_downloads_and_cache_hits() {
        with_cache("checksum", |cache| {
            let platform = layout::Platform::from_triple("x86_64-pc-windows-msvc").unwrap();
            let release = fake_release();
            let hash = format!("{:x}", Sha256::digest(b"some other archive"));

            let result = load(&platform, true, cache, &hash, || Ok(release.clone()));
            assert!(matches!(result, Err(e) if e.contains("SHA-256 mismatch")));
            assert!(!cache.exists());

            fs::create_dir_all(cache.parent().unwrap()).unwrap();
            fs::write(cache, release).unwrap();
            let result = load(&platform, true, cache, &hash, || panic!("overwrote a bad cache hit"));
            assert!(matches!(result, Err(e) if e.contains("SHA-256 mismatch") && e.contains("delete it")));
        });
    }

    #[test]
    fn opt_out_and_other_platforms_skip_the_dxc_download() {
        use serde_json::json;

        with_cache("skip", |cache| {
            let parsed = layout::Metadata::from_cargo(&json!({"ponko": {"windows-dxc": false}}), None).unwrap();
            for (triple, enabled) in [
                ("x86_64-pc-windows-msvc", parsed.windows_dxc.unwrap_or(true)),
                ("aarch64-apple-darwin", true),
            ] {
                let platform = layout::Platform::from_triple(triple).unwrap();
                let result = load(&platform, enabled, cache, SHA256, || panic!("unexpected download"));
                assert!(result.unwrap().is_none());
            }
            assert!(!cache.exists());
        });
    }
}
