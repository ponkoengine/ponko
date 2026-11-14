//! Feature `steam`: steamworks-sys puts the Steam library (libsteam_api)
//! next to binaries in `target/<profile>`, but examples and tests run from
//! `target/<profile>/examples` and `target/<profile>/deps`, where it is
//! missing. Copy it there too, so `cargo run --example` and `cargo test`
//! just work.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // `cargo fuzz` builds with `--cfg fuzzing` (the `fuzz/` targets).
    println!("cargo::rustc-check-cfg=cfg(fuzzing)");
    if std::env::var_os("CARGO_FEATURE_STEAM").is_none() {
        return;
    }
    let Some(out) = std::env::var_os("OUT_DIR") else { return };
    // OUT_DIR is target/<profile>/build/ponko-<hash>/out.
    let Some(profile) = std::path::Path::new(&out).ancestors().nth(3) else {
        return;
    };
    let names = [
        "libsteam_api.dylib",
        "libsteam_api.so",
        "steam_api64.dll",
        "steam_api.dll",
    ];
    let Ok(builds) = std::fs::read_dir(profile.join("build")) else {
        return;
    };
    for dir in builds.flatten() {
        if !dir.file_name().to_string_lossy().starts_with("steamworks-sys-") {
            continue;
        }
        for name in names {
            let lib = dir.path().join("out").join(name);
            if lib.exists() {
                for to in ["examples", "deps"] {
                    let to = profile.join(to);
                    let _ = std::fs::create_dir_all(&to);
                    let _ = std::fs::copy(&lib, to.join(name));
                }
            }
        }
    }
}
