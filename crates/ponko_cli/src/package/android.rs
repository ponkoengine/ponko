//! Android packages: the SDK and NDK found, the environment that builds a
//! game for Android with the NDK's compilers and links it as the library
//! NativeActivity loads, and the apk made of a package's files with
//! `aapt2`, aligned here and signed with `apksigner`.

use super::archive::{self, ApkFile};
use super::layout::ANDROID_MIN_SDK;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The Android tools a package needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tools {
    /// `build-tools/<newest>`: aapt2 and apksigner.
    pub build_tools: PathBuf,
    /// `platforms/android-<newest>/android.jar`, what aapt2 links against.
    pub android_jar: PathBuf,
    /// The NDK, for building (not needed with `--no-build`).
    pub ndk: Option<PathBuf>,
}

/// What the library exports: the C `main` the engine's `android_main`
/// calls, and NativeActivity's entry point. Nothing else is visible.
pub const EXPORTS: &str = "{ global: main; ANativeActivity_onCreate; local: *; };\n";

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// The numbers of a version folder (`36.1.0`, `android-36`, `29.0.14206865`)
/// for sorting: newest last.
fn version_key(name: &str) -> Vec<u64> {
    name.trim_start_matches("android-")
        .split(['.', '-'])
        .map_while(|part| part.parse().ok())
        .collect()
}

/// The newest folder in `dir` (by [`version_key`]) that holds `file`.
fn newest(dir: &Path, file: &str) -> Option<PathBuf> {
    let mut found: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join(file).is_file())
        .filter_map(|p| {
            let key = version_key(&p.file_name()?.to_string_lossy());
            (!key.is_empty()).then_some((key, p))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, p)| p)
}

/// The SDK: `ANDROID_HOME`, `ANDROID_SDK_ROOT`, else where Android
/// Studio puts it.
fn sdk_dir(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(dir) = env(key).filter(|d| !d.is_empty()) {
            return Some(dir.into());
        }
    }
    let home = env("HOME").or_else(|| env("USERPROFILE"))?;
    let default = if cfg!(target_os = "macos") {
        Path::new(&home).join("Library/Android/sdk")
    } else if cfg!(windows) {
        Path::new(&env("LOCALAPPDATA")?).join("Android").join("Sdk")
    } else {
        Path::new(&home).join("Android/Sdk")
    };
    default.is_dir().then_some(default)
}

/// Finds the SDK's newest build tools and platform, and the NDK
/// (`ANDROID_NDK_HOME`, `ANDROID_NDK_ROOT`, else the SDK's newest `ndk/`).
pub fn find(env: &dyn Fn(&str) -> Option<String>) -> Result<Tools, String> {
    let sdk = sdk_dir(env).ok_or(
        "no Android SDK: install the command-line tools and set ANDROID_HOME to the SDK \
         (the guide's Android chapter lists the packages)",
    )?;
    let build_tools = newest(&sdk.join("build-tools"), &format!("aapt2{EXE}")).ok_or_else(|| {
        format!(
            "no build tools in {}: `sdkmanager \"build-tools;36.1.0\"`",
            sdk.display()
        )
    })?;
    let platform = newest(&sdk.join("platforms"), "android.jar")
        .filter(|p| {
            version_key(&p.file_name().unwrap_or_default().to_string_lossy())
                .first()
                .is_some_and(|&api| api >= u64::from(ANDROID_MIN_SDK))
        })
        .ok_or_else(|| {
            format!(
                "no Android platform of API {ANDROID_MIN_SDK} or later in {}: `sdkmanager \"platforms;android-36\"`",
                sdk.display()
            )
        })?;
    let ndk = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]
        .iter()
        .find_map(|k| env(k).filter(|d| !d.is_empty()).map(PathBuf::from))
        .or_else(|| newest(&sdk.join("ndk"), "source.properties"));
    Ok(Tools {
        build_tools,
        android_jar: platform.join("android.jar"),
        ndk,
    })
}

/// The NDK's compilers and tools (`toolchains/llvm/prebuilt/<host>/bin`).
fn ndk_bin(ndk: &Path) -> Result<PathBuf, String> {
    let prebuilt = ndk.join("toolchains/llvm/prebuilt");
    let host = std::fs::read_dir(&prebuilt)
        .ok()
        .and_then(|mut d| d.find_map(Result::ok))
        .ok_or_else(|| format!("{} is not an NDK (no {})", ndk.display(), prebuilt.display()))?;
    Ok(host.path().join("bin"))
}

/// The variables that build `triple` with the NDK: its clang (for the
/// oldest Android a package runs on) as C and C++ compiler and as Rust's
/// linker, and its `llvm-ar`. The caller keeps any the user set.
pub fn build_env(tools: &Tools, triple: &str) -> Result<Vec<(String, String)>, String> {
    let ndk = tools.ndk.as_ref().ok_or(
        "no Android NDK: `sdkmanager \"ndk;29.0.14206865\"`, or set ANDROID_NDK_HOME (the guide's Android chapter)",
    )?;
    let bin = ndk_bin(ndk)?;
    // clang's name for 32-bit ARM is armv7a, Rust's armv7.
    let target = triple.replace("armv7-", "armv7a-").replace("thumbv7neon-", "armv7a-");
    let script = if cfg!(windows) { ".cmd" } else { "" };
    let clang = bin.join(format!("{target}{ANDROID_MIN_SDK}-clang{script}"));
    if !clang.is_file() {
        return Err(format!(
            "the NDK in {} has no {}: use NDK r26 or later",
            ndk.display(),
            clang.display()
        ));
    }
    let under = triple.replace('-', "_");
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    Ok(vec![
        (format!("CC_{under}"), path(clang.clone())),
        (
            format!("CXX_{under}"),
            path(bin.join(format!("{target}{ANDROID_MIN_SDK}-clang++{script}"))),
        ),
        (format!("AR_{under}"), path(bin.join(format!("llvm-ar{EXE}")))),
        (
            format!("CARGO_TARGET_{}_LINKER", under.to_ascii_uppercase()),
            path(clang),
        ),
        ("ANDROID_NDK_HOME".into(), path(ndk.clone())),
    ])
}

/// What `rustc` links the game's program with on Android (after `cargo
/// rustc --`): a shared library (`libmain.so`) that keeps NativeActivity's
/// entry point from the engine and exports only [`EXPORTS`], on 16 KiB
/// pages (which newer phones use, and 4 KiB phones load too).
pub fn link_args(exports: &Path) -> Vec<String> {
    [
        "link-arg=-shared".to_string(),
        "link-arg=-Wl,-soname,libmain.so".into(),
        "link-arg=-Wl,-u,ANativeActivity_onCreate".into(),
        format!("link-arg=-Wl,--version-script={}", exports.display()),
        "link-arg=-Wl,-z,max-page-size=16384".into(),
        // rustc asks for a position-independent executable too, which
        // clang ignores for a library: not worth a warning.
        "link-arg=-Wno-unused-command-line-argument".into(),
    ]
    .into_iter()
    .flat_map(|a| ["-C".to_string(), a])
    .collect()
}

/// Strips the library's symbols in the package, as Gradle does: the
/// build's copy keeps them, to read crash reports with. `false` without
/// an NDK (`--no-build` on a machine without one).
pub fn strip(tools: &Tools, library: &Path) -> Result<bool, String> {
    let Some(ndk) = &tools.ndk else {
        return Ok(false);
    };
    let mut cmd = Command::new(ndk_bin(ndk)?.join(format!("llvm-strip{EXE}")));
    cmd.arg("--strip-unneeded").arg(library);
    run(cmd, "llvm-strip")?;
    Ok(true)
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    let output = cmd.output().map_err(|e| format!("can't run {what}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{what} failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Every file below `dir`, as (`/`-separated name, path), sorted.
fn files_below(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = format!("{rel}{}", e.file_name().to_string_lossy());
        if e.path().is_dir() {
            files_below(&e.path(), &format!("{name}/"), out)?;
        } else {
            out.push((name, e.path()));
        }
    }
    Ok(())
}

/// The debug key Android Studio and Gradle sign test builds with
/// (`~/.android/debug.keystore`, or in `ANDROID_USER_HOME`), made the way
/// they make it when there is none.
fn debug_keystore(env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    let dir = match env("ANDROID_USER_HOME").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = env("HOME")
                .or_else(|| env("USERPROFILE"))
                .ok_or("no home folder for the debug keystore")?;
            Path::new(&home).join(".android")
        }
    };
    let keystore = dir.join("debug.keystore");
    if keystore.is_file() {
        return Ok(keystore);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let keytool = env("JAVA_HOME")
        .map(|j| Path::new(&j).join("bin").join(format!("keytool{EXE}")))
        .filter(|k| k.is_file())
        .unwrap_or_else(|| "keytool".into());
    println!("ponko package: making the debug key {}", keystore.display());
    let mut cmd = Command::new(keytool);
    cmd.args(["-genkeypair", "-noprompt", "-keystore"])
        .arg(&keystore)
        .args([
            "-storepass",
            "android",
            "-keypass",
            "android",
            "-alias",
            "androiddebugkey",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
            "-dname",
            "CN=Android Debug,O=Android,C=US",
        ]);
    run(cmd, "keytool (a JDK, 17 or later)")?;
    Ok(keystore)
}

/// Makes the apk `file` of the package files in `app` (the manifest,
/// `lib/`, `assets/`, `res/`): aapt2 compiles the manifest and icons, the
/// rest goes in as it is, aligned. Signed with the debug key when
/// `debug_sign`, else left unsigned for the `--sign` program.
pub fn apk(tools: &Tools, app: &Path, file: &Path, debug_sign: bool) -> Result<(), String> {
    let parts = file.with_extension("apk-parts");
    let _ = std::fs::remove_dir_all(&parts);
    std::fs::create_dir_all(&parts).map_err(|e| format!("{}: {e}", parts.display()))?;
    let result = (|| {
        let aapt2 = tools.build_tools.join(format!("aapt2{EXE}"));
        let base = parts.join("base.apk");
        let mut link = Command::new(&aapt2);
        link.arg("link")
            .arg("-o")
            .arg(&base)
            .arg("-I")
            .arg(&tools.android_jar)
            .arg("--manifest")
            .arg(app.join("AndroidManifest.xml"));
        let res = app.join("res");
        if res.is_dir() {
            let compiled = parts.join("res.zip");
            let mut compile = Command::new(&aapt2);
            compile.arg("compile").arg("--dir").arg(&res).arg("-o").arg(&compiled);
            run(compile, "aapt2 compile")?;
            link.arg(&compiled);
        }
        run(link, "aapt2 link")?;
        let mut files = archive::unzip(&std::fs::read(&base).map_err(|e| format!("{}: {e}", base.display()))?)?;
        for dir in ["lib", "assets"] {
            let mut found = Vec::new();
            if app.join(dir).is_dir() {
                files_below(&app.join(dir), &format!("{dir}/"), &mut found)?;
            }
            for (name, path) in found {
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                files.push(ApkFile { name, bytes });
            }
        }
        let unsigned = if debug_sign {
            parts.join("unsigned.apk")
        } else {
            file.to_path_buf()
        };
        std::fs::write(&unsigned, archive::apk_bytes(&files)?).map_err(|e| format!("{}: {e}", unsigned.display()))?;
        if debug_sign {
            let env = |k: &str| std::env::var(k).ok();
            let keystore = debug_keystore(&env)?;
            let script = if cfg!(windows) { ".bat" } else { "" };
            let mut sign = Command::new(tools.build_tools.join(format!("apksigner{script}")));
            sign.args(["sign", "--ks"])
                .arg(&keystore)
                .args([
                    "--ks-pass",
                    "pass:android",
                    "--key-pass",
                    "pass:android",
                    "--ks-key-alias",
                    "androiddebugkey",
                    "--out",
                ])
                .arg(file)
                .arg(&unsigned);
            run(sign, "apksigner (needs a JDK, 17 or later)")?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&parts);
    let _ = std::fs::remove_file(file.with_extension("apk.idsig"));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_build_tools_platform_and_ndk_are_found() {
        let root = std::env::temp_dir().join(format!("ponko-android-sdk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let touch = |rel: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        };
        touch(&format!("build-tools/34.0.0/aapt2{EXE}"));
        touch(&format!("build-tools/36.1.0/aapt2{EXE}"));
        touch(&format!("build-tools/36.0.0/aapt2{EXE}"));
        touch("build-tools/37.0.0-rc1/README"); // no aapt2: not a candidate
        touch("platforms/android-34/android.jar");
        touch("platforms/android-36/android.jar");
        touch("ndk/27.2.12479018/source.properties");
        touch("ndk/29.0.14206865/source.properties");
        let sdk = root.to_string_lossy().into_owned();
        let env = |k: &str| (k == "ANDROID_HOME").then(|| sdk.clone());
        let tools = find(&env).unwrap();
        assert_eq!(tools.build_tools, root.join("build-tools/36.1.0"));
        assert_eq!(tools.android_jar, root.join("platforms/android-36/android.jar"));
        assert_eq!(tools.ndk, Some(root.join("ndk/29.0.14206865")));
        // ANDROID_NDK_HOME names the NDK over the SDK's.
        let env = |k: &str| match k {
            "ANDROID_HOME" => Some(sdk.clone()),
            "ANDROID_NDK_HOME" => Some("/opt/ndk".into()),
            _ => None,
        };
        assert_eq!(find(&env).unwrap().ndk, Some(PathBuf::from("/opt/ndk")));
        // A platform older than the oldest Android a package runs on is refused.
        std::fs::remove_dir_all(root.join("platforms/android-36")).unwrap();
        std::fs::remove_dir_all(root.join("platforms/android-34")).unwrap();
        touch("platforms/android-29/android.jar");
        let env = |k: &str| (k == "ANDROID_HOME").then(|| sdk.clone());
        assert!(find(&env).unwrap_err().contains("API 30"));
        let _ = std::fs::remove_dir_all(&root);
        let nothing = |_: &str| None;
        assert!(find(&nothing).unwrap_err().contains("ANDROID_HOME"));
    }

    #[test]
    fn the_ndk_clang_builds_and_links_for_the_oldest_supported_android() {
        let root = std::env::temp_dir().join(format!("ponko-android-ndk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("toolchains/llvm/prebuilt/some-host/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = if cfg!(windows) { ".cmd" } else { "" };
        std::fs::write(bin.join(format!("aarch64-linux-android30-clang{script}")), b"").unwrap();
        let tools = Tools {
            build_tools: PathBuf::new(),
            android_jar: PathBuf::new(),
            ndk: Some(root.clone()),
        };
        let env = build_env(&tools, "aarch64-linux-android").unwrap();
        let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap();
        let clang = bin.join(format!("aarch64-linux-android30-clang{script}"));
        assert_eq!(get("CC_aarch64_linux_android"), clang.to_string_lossy());
        assert_eq!(
            get("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"),
            clang.to_string_lossy()
        );
        assert!(get("AR_aarch64_linux_android").contains("llvm-ar"));
        // An NDK without that compiler (too old) is named.
        assert!(
            build_env(&tools, "x86_64-linux-android")
                .unwrap_err()
                .contains("x86_64-linux-android30-clang")
        );
        let no_ndk = Tools { ndk: None, ..tools };
        assert!(build_env(&no_ndk, "aarch64-linux-android").unwrap_err().contains("NDK"));
        let _ = std::fs::remove_dir_all(&root);
        let args = link_args(Path::new("/t/exports.map"));
        assert!(args.windows(2).any(|w| w == ["-C", "link-arg=-shared"]));
        assert!(args.contains(&"link-arg=-Wl,--version-script=/t/exports.map".to_string()));
    }
}
