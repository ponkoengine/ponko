//! What goes where in a package, for each platform: pure functions of the
//! game's metadata and the built program, tested for every platform on
//! any machine. [`super`] builds, writes the plan to disk and archives it.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// The operating system a package is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
    /// A browser: `wasm32-unknown-unknown`.
    Web,
    /// iPhones and iPads: `aarch64-apple-ios`.
    Ios,
    /// The iOS simulator on a Mac: `aarch64-apple-ios-sim` (or
    /// `x86_64-apple-ios` on an Intel Mac).
    IosSimulator,
    /// Android phones and tablets: `aarch64-linux-android` (and the other
    /// Android triples, which are built but untested).
    Android,
}

impl Os {
    /// iOS, on a device or the simulator.
    pub fn is_ios(self) -> bool {
        matches!(self, Os::Ios | Os::IosSimulator)
    }
}

/// The triples that `--target` platform aliases stand for.
pub fn target_alias(target: &str) -> &str {
    match target {
        "windows" => "x86_64-pc-windows-msvc",
        "ios" => "aarch64-apple-ios",
        "ios-sim" | "ios-simulator" => "aarch64-apple-ios-sim",
        "android" => "aarch64-linux-android",
        other => other,
    }
}

/// The oldest iOS a package runs on (`MinimumOSVersion`, and the
/// `IPHONEOS_DEPLOYMENT_TARGET` it is built for): iOS 16 runs on every
/// iPhone from the iPhone 8 and every iPad with an A9X or later, all with
/// Metal.
pub const IOS_MINIMUM: &str = "16.0";

/// The oldest Android a package runs on (`minSdkVersion`, and the API level
/// the NDK links it for): Android 11. It is the first with the
/// `WindowInsets.Type` calls the safe area and the keyboard inset come
/// from, and every phone it runs on has Vulkan 1.1 or OpenGL ES 3.2.
pub const ANDROID_MIN_SDK: u32 = 30;

/// The Android the package is made for (`targetSdkVersion`): Android 16,
/// what it was checked on. App stores ask for a recent one.
pub const ANDROID_TARGET_SDK: u32 = 36;

/// The folder of an Android CPU's libraries in an apk (its ABI), from the
/// triple's first part.
pub fn android_abi(arch: &str) -> Option<&'static str> {
    match arch {
        "aarch64" => Some("arm64-v8a"),
        "x86_64" => Some("x86_64"),
        "armv7" | "thumbv7neon" => Some("armeabi-v7a"),
        "i686" => Some("x86"),
        _ => None,
    }
}

/// The platform a package is for, from a Rust target triple.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    /// The CPU (`x86_64`, `aarch64`, ...), the triple's first part.
    pub arch: String,
    pub triple: String,
}

impl Platform {
    pub fn from_triple(triple: &str) -> Result<Platform, String> {
        let arch = triple.split('-').next().unwrap_or_default().to_string();
        let os = if triple.starts_with("wasm32-unknown-unknown") {
            Os::Web
        } else if triple.contains("-windows") {
            Os::Windows
        } else if triple.contains("-apple-darwin") {
            Os::Mac
        } else if triple.contains("-apple-ios") && !triple.contains("macabi") {
            if triple.ends_with("-sim") || triple.starts_with("x86_64-") {
                Os::IosSimulator
            } else {
                Os::Ios
            }
        } else if triple.contains("-linux-android") {
            if android_abi(&arch).is_none() {
                return Err(format!("can't package for `{triple}`: Android has no ABI for `{arch}`"));
            }
            Os::Android
        } else if triple.contains("-linux") {
            Os::Linux
        } else {
            return Err(format!(
                "can't package for `{triple}`: packages are made for Windows, macOS, Linux, iOS (`--target ios`, `--target ios-sim`), Android (`--target android`) and the web (wasm32-unknown-unknown)"
            ));
        };
        Ok(Platform {
            os,
            arch,
            triple: triple.to_string(),
        })
    }

    /// The end of the package's name: `windows-x86_64`, `macos-aarch64`, `web`.
    pub fn label(&self) -> String {
        match self.os {
            Os::Web => "web".into(),
            Os::Windows => format!("windows-{}", self.arch),
            Os::Mac => format!("macos-{}", self.arch),
            Os::Linux => format!("linux-{}", self.arch),
            Os::Ios => format!("ios-{}", self.arch),
            Os::IosSimulator => format!("ios-simulator-{}", self.arch),
            Os::Android => format!("android-{}", self.arch),
        }
    }

    /// The program's file name for a Cargo target called `name`.
    /// The file of the dynamic library `name` (as `-l` names it) on this
    /// platform: `libname.dylib`, `libname.so`, `name.dll`; none on the web.
    pub fn library_name(&self, name: &str) -> Option<String> {
        match self.os {
            Os::Windows => Some(format!("{name}.dll")),
            Os::Mac | Os::Ios | Os::IosSimulator => Some(format!("lib{name}.dylib")),
            Os::Linux | Os::Android => Some(format!("lib{name}.so")),
            Os::Web => None,
        }
    }

    pub fn exe_name(&self, name: &str) -> String {
        match self.os {
            Os::Windows => format!("{name}.exe"),
            Os::Web => format!("{name}.wasm"),
            _ => name.to_string(),
        }
    }
}

/// What a package is made into, after its folder is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Format {
    /// Only the plain folder: the program next to its files, also on macOS
    /// (no app bundle), and no archive.
    Folder,
    Zip,
    TarGz,
    /// A macOS disk image (needs `hdiutil`, so a Mac).
    Dmg,
    /// An iOS app archive: a zip of `Payload/Name.app`, what a device
    /// installs (once signed).
    Ipa,
    /// An Android package: what a phone installs, signed with the debug
    /// key or by the `--sign` program.
    Apk,
    /// An Android App Bundle: what Google Play takes, made with Google's
    /// `bundletool`; signed like the apk.
    Aab,
}

impl Format {
    pub fn parse_list(list: &str) -> Result<Vec<Format>, String> {
        let mut out = BTreeSet::new();
        for word in list.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            out.insert(match word {
                "folder" => Format::Folder,
                "zip" => Format::Zip,
                "tar.gz" | "tgz" => Format::TarGz,
                "dmg" => Format::Dmg,
                "ipa" => Format::Ipa,
                "apk" => Format::Apk,
                "aab" => Format::Aab,
                other => {
                    return Err(format!(
                        "unknown format `{other}`: use folder, zip, tar.gz, dmg, ipa, apk or aab (comma-separated)"
                    ));
                }
            });
        }
        if out.is_empty() {
            return Err("--format needs at least one of folder, zip, tar.gz, dmg, ipa, apk, aab".into());
        }
        Ok(out.into_iter().collect())
    }

    /// What a platform's players usually download. `dmg` is possible only
    /// where `hdiutil` is (a Mac); without it a macOS app is zipped.
    pub fn defaults(os: Os, can_dmg: bool) -> Vec<Format> {
        match os {
            Os::Windows => vec![Format::Zip],
            Os::Linux => vec![Format::TarGz],
            Os::Mac if can_dmg => vec![Format::Dmg],
            Os::Mac => vec![Format::Zip],
            Os::Web => vec![Format::Folder],
            Os::Ios => vec![Format::Ipa],
            // `xcrun simctl install` takes the app folder; the zip carries it.
            Os::IosSimulator => vec![Format::Zip],
            Os::Android => vec![Format::Apk],
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Folder => "",
            Format::Zip => "zip",
            Format::TarGz => "tar.gz",
            Format::Dmg => "dmg",
            Format::Ipa => "ipa",
            Format::Apk => "apk",
            Format::Aab => "aab",
        }
    }
}

/// Checks formats against the platform; returns whether the package is an
/// app bundle (the default for macOS, always for iOS) and the archives to make.
pub fn check_formats(platform: &Platform, formats: &[Format]) -> Result<(bool, Vec<Format>), String> {
    if formats.contains(&Format::Dmg) && platform.os != Os::Mac {
        return Err(format!("a dmg is for macOS, not {}", platform.triple));
    }
    match platform.os {
        Os::Ios => {}
        Os::IosSimulator if formats.contains(&Format::Ipa) => {
            return Err("an ipa is for devices: the simulator installs the app folder (use folder or zip)".into());
        }
        _ if formats.contains(&Format::Ipa) => {
            return Err(format!("an ipa is for iOS, not {}", platform.triple));
        }
        _ => {}
    }
    if platform.os.is_ios() && formats.contains(&Format::TarGz) {
        return Err("iOS packages are zips (an ipa, or the app zipped for the simulator), not tar.gz".into());
    }
    if platform.os == Os::Android {
        if let Some(other) = formats
            .iter()
            .find(|f| !matches!(f, Format::Apk | Format::Aab | Format::Folder))
        {
            return Err(format!(
                "an Android package is an apk or an app bundle, aab (or `folder`, the files they are made of), not {}",
                other.extension()
            ));
        }
    } else if let Some(android) = formats.iter().find(|f| matches!(f, Format::Apk | Format::Aab)) {
        return Err(format!(
            "an {} is for Android, not {}",
            android.extension(),
            platform.triple
        ));
    }
    let flat = formats.contains(&Format::Folder);
    if flat && formats.contains(&Format::Dmg) {
        return Err("a dmg holds the app bundle: don't combine it with `folder`".into());
    }
    // An iOS app is always a bundle, and an Android package's files are
    // always laid out for the apk: `folder` keeps the folder holding them.
    let app = (platform.os == Os::Mac && !flat) || platform.os.is_ios() || platform.os == Os::Android;
    let archives = formats.iter().copied().filter(|f| *f != Format::Folder).collect();
    Ok((app, archives))
}

/// A file copied into the package: where it is, and its path in the package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCopy {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// The game, as its `[package.metadata.ponko]` table and Cargo describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    /// The name players see: the app's name, the window of a web page.
    pub name: String,
    /// The name in file names: `coin-dash` for "Coin Dash".
    pub slug: String,
    /// The reverse-DNS app id: the macOS and iOS bundle identifier, the
    /// Android package name (with `-` as `_`).
    pub id: String,
    pub version: String,
    /// The Cargo target (binary or example) that is the program.
    pub target: String,
    /// A square PNG.
    pub icon: Option<PathBuf>,
    /// Every asset file, at the same path relative to the package as to the
    /// game's `Cargo.toml` (so `assets/...` stays `assets/...`).
    pub assets: Vec<FileCopy>,
    /// Licenses, readmes and notices, at the package's top level.
    pub files: Vec<FileCopy>,
    /// Which ways a phone or tablet may be held (iOS and Android).
    pub orientation: Orientation,
    /// Include the DX12 shader compiler in Windows packages.
    pub windows_dxc: bool,
}

/// The `orientation` key: which ways a phone or tablet (iOS, Android) may
/// be held while the game runs. Desktops and the web ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Every way (upright, and sideways either way; iPads upside down too).
    #[default]
    Any,
    /// Sideways only, either way.
    Landscape,
    /// Upright only (iPads upside down too).
    Portrait,
}

impl Orientation {
    fn parse(word: &str) -> Result<Orientation, String> {
        match word {
            "any" => Ok(Orientation::Any),
            "landscape" => Ok(Orientation::Landscape),
            "portrait" => Ok(Orientation::Portrait),
            other => Err(format!(
                "unknown orientation `{other}` (use any, landscape or portrait)"
            )),
        }
    }

    /// The activity's `android:screenOrientation`: the `user` values follow
    /// the player's rotation lock, as other apps do.
    fn android_value(self) -> &'static str {
        match self {
            Orientation::Any => "fullUser",
            Orientation::Landscape => "userLandscape",
            Orientation::Portrait => "userPortrait",
        }
    }

    /// `UISupportedInterfaceOrientations` for an iPhone (no upside down)
    /// or an iPad.
    fn ios_values(self, ipad: bool) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self != Orientation::Landscape {
            out.push("UIInterfaceOrientationPortrait");
            if ipad {
                out.push("UIInterfaceOrientationPortraitUpsideDown");
            }
        }
        if self != Orientation::Portrait {
            out.extend([
                "UIInterfaceOrientationLandscapeLeft",
                "UIInterfaceOrientationLandscapeRight",
            ]);
        }
        out
    }
}

/// The `[package.metadata.ponko]` keys, before files are looked up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub name: Option<String>,
    pub id: Option<String>,
    pub icon: Option<String>,
    pub assets: Option<Vec<String>>,
    /// (source, name in the package)
    pub files: Option<Vec<(String, String)>>,
    /// How the files under `assets/` ship: `pack = false` loose, else in
    /// one `assets.pack` (the default).
    pub pack: Option<PackMeta>,
    /// `orientation`: `any` (the default), `landscape` or `portrait`.
    pub orientation: Option<Orientation>,
    /// Windows packages bundle DXC unless explicitly disabled.
    pub windows_dxc: Option<bool>,
}

/// The `pack` key: `true`, `false` or `{ exclude = [...], textures = true,
/// cook = false }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackMeta {
    pub enabled: bool,
    /// Patterns of files left out of the pack (and the package).
    pub exclude: Vec<String>,
    /// Convert images to KTX2 textures per `textures.json`, as `ponko pack --textures`.
    pub textures: bool,
    /// Cook the files (`ponko cook`) into the pack: on by default.
    pub cook: bool,
}

impl Default for PackMeta {
    fn default() -> Self {
        PackMeta {
            enabled: true,
            exclude: Vec::new(),
            textures: false,
            cook: true,
        }
    }
}

const KEYS: &[&str] = &[
    "name",
    "id",
    "icon",
    "assets",
    "files",
    "pack",
    "orientation",
    "windows-dxc",
    "examples",
];

impl Metadata {
    /// Reads the package's `metadata` value (from `cargo metadata`): the
    /// `ponko` table, with `ponko.examples.<example>` on top for an example.
    pub fn from_cargo(metadata: &Value, example: Option<&str>) -> Result<Metadata, String> {
        let table = metadata.get("ponko");
        let mut out = Metadata::default();
        if let Some(table) = table {
            out.merge(table, "[package.metadata.ponko]")?;
        }
        if let Some(example) = example {
            let key = format!("[package.metadata.ponko.examples.{example}]");
            if let Some(t) = table.and_then(|t| t.get("examples")).and_then(|e| e.get(example)) {
                out.merge(t, &key)?;
            }
        }
        Ok(out)
    }

    fn merge(&mut self, table: &Value, at: &str) -> Result<(), String> {
        let object = table.as_object().ok_or_else(|| format!("{at} is not a table"))?;
        for key in object.keys() {
            if !KEYS.contains(&key.as_str()) || (key == "examples" && at.contains(".examples.")) {
                return Err(format!(
                    "{at}: unknown key `{key}` (the keys are name, id, icon, assets, files, pack, orientation, windows-dxc{})",
                    if at.contains(".examples.") { "" } else { " and examples" }
                ));
            }
        }
        let string = |key: &str| -> Result<Option<String>, String> {
            match object.get(key) {
                None => Ok(None),
                Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.clone())),
                Some(_) => Err(format!("{at} {key}: expected a non-empty string")),
            }
        };
        if let Some(v) = string("name")? {
            self.name = Some(v);
        }
        if let Some(v) = string("id")? {
            check_id(&v).map_err(|e| format!("{at} id: {e}"))?;
            self.id = Some(v);
        }
        if let Some(v) = string("icon")? {
            self.icon = Some(v);
        }
        if let Some(v) = string("orientation")? {
            self.orientation = Some(Orientation::parse(&v).map_err(|e| format!("{at} orientation: {e}"))?);
        }
        if let Some(v) = object.get("windows-dxc") {
            self.windows_dxc = Some(
                v.as_bool()
                    .ok_or_else(|| format!("{at} windows-dxc: expected true or false"))?,
            );
        }
        if let Some(v) = object.get("assets") {
            let list = v
                .as_array()
                .ok_or_else(|| format!("{at} assets: expected a list of folders or files"))?;
            let mut assets = Vec::new();
            for item in list {
                let s = item
                    .as_str()
                    .ok_or_else(|| format!("{at} assets: expected strings, got {item}"))?;
                assets.push(s.to_string());
            }
            self.assets = Some(assets);
        }
        if let Some(v) = object.get("files") {
            let list = v.as_array().ok_or_else(|| format!("{at} files: expected a list"))?;
            let mut files = Vec::new();
            for item in list {
                files.push(match item {
                    Value::String(s) => (s.clone(), file_name(s)?),
                    Value::Object(o) => {
                        let get = |k: &str| {
                            o.get(k)
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .ok_or_else(|| format!("{at} files: a table needs `from` and `to` strings, got {item}"))
                        };
                        let to = get("to")?;
                        (get("from")?, files_to(&to).map_err(|e| format!("{at} files: {e}"))?)
                    }
                    other => return Err(format!("{at} files: expected a path or {{ from, to }}, got {other}")),
                });
            }
            self.files = Some(files);
        }
        if let Some(v) = object.get("pack") {
            self.pack = Some(match v {
                Value::Bool(enabled) => PackMeta {
                    enabled: *enabled,
                    ..PackMeta::default()
                },
                Value::Object(o) => {
                    let mut pack = PackMeta::default();
                    for (key, value) in o {
                        match key.as_str() {
                            "exclude" => {
                                pack.exclude = value
                                    .as_array()
                                    .and_then(|list| list.iter().map(|p| p.as_str().map(str::to_string)).collect())
                                    .ok_or_else(|| format!("{at} pack.exclude: expected a list of patterns"))?;
                            }
                            "textures" => {
                                pack.textures = value
                                    .as_bool()
                                    .ok_or_else(|| format!("{at} pack.textures: expected true or false"))?;
                            }
                            "cook" => {
                                pack.cook = value
                                    .as_bool()
                                    .ok_or_else(|| format!("{at} pack.cook: expected true or false"))?;
                            }
                            other => {
                                return Err(format!(
                                    "{at} pack: unknown key `{other}` (the keys are exclude, textures and cook)"
                                ));
                            }
                        }
                    }
                    pack
                }
                other => return Err(format!("{at} pack: expected true, false or a table, got {other}")),
            });
        }
        Ok(())
    }
}

/// A `files` entry's `to`: a file name, or a path below the package's top
/// folder (`docs/README.txt`), `/`-separated, with no `.`, `..` or empty
/// parts. Returned as written.
fn files_to(to: &str) -> Result<String, String> {
    let parts: Vec<&str> = to.split('/').collect();
    if to.contains('\\') {
        return Err(format!("`to = \"{to}\"`: separate folders with `/`"));
    }
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return Err(format!(
            "`to = \"{to}\"` must be a file name or a path inside the package (`docs/README.txt`: relative, no `.` or `..`)"
        ));
    }
    if parts.iter().any(|p| p.contains(':')) {
        return Err(format!("`to = \"{to}\"`: `:` can't be in a file name on every system"));
    }
    Ok(to.to_string())
}

fn file_name(path: &str) -> Result<String, String> {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("`{path}` names no file"))
}

/// An app id is reverse DNS: dot-separated parts of letters, digits, `-`.
pub fn check_id(id: &str) -> Result<(), String> {
    let parts: Vec<&str> = id.split('.').collect();
    let good = parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if good {
        Ok(())
    } else {
        Err(format!(
            "`{id}` is not a reverse-DNS id such as `com.example.my-game` (dot-separated letters, digits and `-`)"
        ))
    }
}

/// `Coin Dash` -> `coin-dash`: lower case, runs of other characters become one `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A relative path inside the game's folder, with no `..`: it is also the
/// file's path in the package.
pub fn package_path(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let ok = !path.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !ok {
        return Err(format!(
            "assets: `{path}` must be a path inside the game's folder (relative, no `..`): it is also its path in the package"
        ));
    }
    Ok(p.components().filter(|c| matches!(c, Component::Normal(_))).collect())
}

/// Where a file comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Copy(PathBuf),
    Text(String),
    Bytes(Vec<u8>),
}

/// One file of the package: its path below the output folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub source: Source,
    pub executable: bool,
}

/// Everything to write: `folder` (below the output folder) holds `entries`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// `<slug>-<version>-<platform>`.
    pub folder: PathBuf,
    /// The macOS app bundle inside `folder`, if any.
    pub app: Option<PathBuf>,
    /// The program, below the output folder (for the smoke run and signing).
    pub program: PathBuf,
    pub entries: Vec<Entry>,
}

/// What the build produced.
pub enum Built<'a> {
    /// A desktop executable.
    Program(&'a Path),
    /// A web build: wasm-bindgen's `game.js` and `game_bg.wasm`, in `dir`;
    /// `threads` when it was built with them (atomics and shared memory);
    /// `isolation_worker` adds the service worker that sends the
    /// cross-origin isolation headers on hosts that can't.
    Web {
        dir: &'a Path,
        threads: bool,
        isolation_worker: bool,
    },
    /// An Android build: the program linked as a shared library, which
    /// Android's NativeActivity loads. `debuggable` when the package is
    /// signed with the debug key (for testing, not for players).
    Android { library: &'a Path, debuggable: bool },
}

/// The name of a package's folder (and archives): `coin-dash-0.1.0-macos-aarch64`.
pub fn folder_name(game: &Game, platform: &Platform) -> String {
    format!("{}-{}-{}", game.slug, game.version, platform.label())
}

/// Plans a package: the program, assets, files and notices, laid out for the
/// platform (a macOS app bundle when `app`). `icon_png` is the icon's bytes.
/// With `pack` (an asset pack of every file under `assets/`), the package
/// holds that pack, `assets.pack`, where the `assets/` folder would be, and
/// the asset files that aren't under `assets/` beside it.
pub fn plan(
    game: &Game,
    platform: &Platform,
    app: bool,
    built: Built,
    notices: &str,
    icon_png: Option<&[u8]>,
    pack: Option<&[u8]>,
) -> Result<Plan, String> {
    let folder = PathBuf::from(folder_name(game, platform));
    let mut entries = Vec::new();
    // The asset files copied as they are: all of them, or those the pack doesn't hold.
    let loose: Vec<&FileCopy> = game
        .assets
        .iter()
        .filter(|a| pack.is_none() || !a.to.starts_with("assets"))
        .collect();
    let assets = |entries: &mut Vec<Entry>, dir: &Path| {
        for a in &loose {
            entries.push(Entry {
                path: dir.join(&a.to),
                source: Source::Copy(a.from.clone()),
                executable: false,
            });
        }
        if let Some(bytes) = pack {
            entries.push(Entry {
                path: dir.join(crate::pack::DEFAULT_PACK),
                source: Source::Bytes(bytes.to_vec()),
                executable: false,
            });
        }
    };
    let top = |name: &str| folder.join(name);
    let docs = |entries: &mut Vec<Entry>, dir: &Path| {
        for f in &game.files {
            entries.push(Entry {
                path: dir.join(&f.to),
                source: Source::Copy(f.from.clone()),
                executable: false,
            });
        }
        entries.push(Entry {
            path: dir.join(NOTICES),
            source: Source::Text(notices.to_string()),
            executable: false,
        });
    };
    let (program, app_dir) = match (built, app) {
        (
            Built::Web {
                dir,
                threads,
                isolation_worker,
            },
            _,
        ) => {
            for name in ["game.js", "game_bg.wasm"] {
                entries.push(Entry {
                    path: top(name),
                    source: Source::Copy(dir.join(name)),
                    executable: false,
                });
            }
            let mut page = web_index(&game.name, icon_png.is_some());
            if pack.is_some() {
                page = with_web_pack(&page);
            }
            if isolation_worker {
                page = with_isolation_worker(&page);
                entries.push(Entry {
                    path: top("ponko-isolate.js"),
                    source: Source::Text(WEB_ISOLATE.to_string()),
                    executable: false,
                });
            }
            entries.push(Entry {
                path: top("index.html"),
                source: Source::Text(page),
                executable: false,
            });
            assets(&mut entries, &folder);
            let loose: Vec<FileCopy> = loose.iter().map(|a| (*a).clone()).collect();
            if let Some(index) = web_asset_index(&loose) {
                entries.push(Entry {
                    path: top("assets/index.json"),
                    source: Source::Text(index),
                    executable: false,
                });
            }
            if let Some(png) = icon_png {
                entries.push(Entry {
                    path: top("icon.png"),
                    source: Source::Bytes(png.to_vec()),
                    executable: false,
                });
            }
            docs(&mut entries, &folder);
            if threads {
                entries.push(Entry {
                    path: top("ponko-worker.js"),
                    source: Source::Text(WEB_WORKER.to_string()),
                    executable: false,
                });
                // The game's own file at one of these places wins.
                for (name, text) in web_header_files() {
                    if !entries.iter().any(|e| e.path == top(name)) {
                        entries.push(Entry {
                            path: top(name),
                            source: Source::Text(text),
                            executable: false,
                        });
                    }
                }
            }
            (top("index.html"), None)
        }
        (Built::Android { library, debuggable }, _) => {
            // The files the apk is made of: the manifest, the library, the
            // pack and the notices under assets/, the icons under res/.
            let app = folder.join("apk");
            if let Some(a) = loose.first() {
                return Err(format!(
                    "{} would be a loose file in an Android package, where the game can't read it: \
                     an Android game reads its assets from assets.pack (so no --no-pack or `pack = false`, \
                     and `assets` only below assets/)",
                    a.to.display()
                ));
            }
            let abi =
                android_abi(&platform.arch).ok_or_else(|| format!("Android has no ABI for `{}`", platform.arch))?;
            let program = app.join("lib").join(abi).join(ANDROID_LIBRARY);
            entries.push(Entry {
                path: program.clone(),
                source: Source::Copy(library.to_path_buf()),
                executable: false,
            });
            assets(&mut entries, &app.join("assets"));
            let icon = match icon_png {
                Some(png) => {
                    for (dir, bytes) in super::archive::android_icons(png)? {
                        entries.push(Entry {
                            path: app.join("res").join(dir).join("ic_launcher.png"),
                            source: Source::Bytes(bytes),
                            executable: false,
                        });
                    }
                    true
                }
                None => false,
            };
            entries.push(Entry {
                path: app.join("AndroidManifest.xml"),
                source: Source::Text(android_manifest(game, debuggable, icon)?),
                executable: false,
            });
            docs(&mut entries, &app.join("assets"));
            docs(&mut entries, &folder);
            (program, Some(app))
        }
        (Built::Program(exe), false) => {
            let program = top(&platform.exe_name(&game.target));
            entries.push(Entry {
                path: program.clone(),
                source: Source::Copy(exe.to_path_buf()),
                executable: true,
            });
            assets(&mut entries, &folder);
            if let Some(png) = icon_png {
                entries.push(Entry {
                    path: top("icon.png"),
                    source: Source::Bytes(png.to_vec()),
                    executable: false,
                });
            }
            docs(&mut entries, &folder);
            (program, None)
        }
        // An iOS app: everything at the bundle's top, the program too.
        (Built::Program(exe), true) if platform.os.is_ios() => {
            let bundle = folder.join(format!("{}.app", file_safe(&game.name)));
            let program = bundle.join(&game.target);
            entries.push(Entry {
                path: program.clone(),
                source: Source::Copy(exe.to_path_buf()),
                executable: true,
            });
            assets(&mut entries, &bundle);
            let icon = match icon_png {
                Some(png) => {
                    for (name, bytes) in super::archive::ios_icons(png)? {
                        entries.push(Entry {
                            path: bundle.join(name),
                            source: Source::Bytes(bytes),
                            executable: false,
                        });
                    }
                    true
                }
                None => false,
            };
            entries.push(Entry {
                path: bundle.join("Info.plist"),
                source: Source::Text(ios_info_plist(game, platform, icon)),
                executable: false,
            });
            docs(&mut entries, &bundle);
            docs(&mut entries, &folder);
            (program, Some(bundle))
        }
        (Built::Program(exe), true) => {
            let bundle = folder.join(format!("{}.app", file_safe(&game.name)));
            let contents = bundle.join("Contents");
            let program = contents.join("MacOS").join(&game.target);
            entries.push(Entry {
                path: program.clone(),
                source: Source::Copy(exe.to_path_buf()),
                executable: true,
            });
            let resources = contents.join("Resources");
            assets(&mut entries, &resources);
            let icon = match icon_png {
                Some(png) => {
                    entries.push(Entry {
                        path: resources.join("icon.icns"),
                        source: Source::Bytes(super::archive::icns(png)?),
                        executable: false,
                    });
                    true
                }
                None => false,
            };
            entries.push(Entry {
                path: contents.join("Info.plist"),
                source: Source::Text(info_plist(game, platform, icon)),
                executable: false,
            });
            // Inside the bundle for the app, and beside it to read before opening.
            docs(&mut entries, &resources);
            docs(&mut entries, &folder);
            (program, Some(bundle))
        }
    };
    let mut seen = BTreeSet::new();
    for e in &entries {
        if !seen.insert(e.path.clone()) {
            return Err(format!(
                "two files would be {} in the package: check `assets` and `files` in [package.metadata.ponko]",
                e.path.display()
            ));
        }
    }
    // A file can't also be a folder another file is in (`files` with
    // `to = "docs/README.txt"` beside a file named `docs`).
    for e in &entries {
        if let Some(folder) = e.path.ancestors().skip(1).find(|a| seen.contains(*a)) {
            return Err(format!(
                "{} would be a file and the folder of {} in the package: check `assets` and `files` in [package.metadata.ponko]",
                folder.display(),
                e.path.display()
            ));
        }
    }
    Ok(Plan {
        folder,
        app: app_dir,
        program,
        entries,
    })
}

/// Adds the dynamic `libraries` the program links beside it (inside a macOS
/// app, in `Contents/MacOS`), where it looks for them.
pub fn with_libraries(mut plan: Plan, libraries: &[PathBuf]) -> Result<Plan, String> {
    let dir = plan.program.parent().map(Path::to_path_buf).unwrap_or_default();
    for lib in libraries {
        let name = lib
            .file_name()
            .ok_or_else(|| format!("{} names no file", lib.display()))?;
        let path = dir.join(name);
        if plan.entries.iter().any(|e| e.path == path) {
            return Err(format!(
                "two files would be {} in the package: a library the program links, and one from [package.metadata.ponko]",
                path.display()
            ));
        }
        plan.entries.push(Entry {
            path,
            source: Source::Copy(lib.clone()),
            executable: false,
        });
    }
    Ok(plan)
}

/// The program's file in an apk (`lib/<abi>/libmain.so`): the name
/// NativeActivity loads by default.
pub const ANDROID_LIBRARY: &str = "libmain.so";

/// The launcher icon's sizes: (folder under `res/`, pixels).
pub const ANDROID_ICONS: &[(&str, u32)] = &[
    ("mipmap-mdpi", 48),
    ("mipmap-hdpi", 72),
    ("mipmap-xhdpi", 96),
    ("mipmap-xxhdpi", 144),
    ("mipmap-xxxhdpi", 192),
];

/// The Android package name for an app id: Java's rules, so each part
/// starts with a letter and `-` becomes `_` (`dev.example.coin-dash` ->
/// `dev.example.coin_dash`).
pub fn android_package(id: &str) -> Result<String, String> {
    let name = id.replace('-', "_");
    let good = name.split('.').count() >= 2
        && name.split('.').all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if good {
        Ok(name)
    } else {
        Err(format!(
            "the id `{id}` can't be an Android package name: every dot-separated part must start with a letter \
             (set `id` in [package.metadata.ponko], such as `com.example.my-game`)"
        ))
    }
}

/// The `versionCode` of a Cargo version:
/// `major * 1000000 + minor * 1000 + patch` (`1.2.3` -> `1002003`), so a
/// later version always has a larger code, as app stores require. A pre-release has its release's code.
pub fn android_version_code(version: &str) -> Result<u32, String> {
    let numbers: Vec<u64> = bundle_version(version)
        .split('.')
        .map(|n| n.parse::<u64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("version `{version}` is not major.minor.patch"))?;
    let [major, minor, patch] = numbers[..] else {
        return Err(format!("version `{version}` is not major.minor.patch"));
    };
    let code = major * 1_000_000 + minor * 1_000 + patch;
    if minor >= 1000 || patch >= 1000 || code == 0 || code > 2_100_000_000 {
        return Err(format!(
            "version `{version}` has no Android versionCode: it is major * 1000000 + minor * 1000 + patch, \
             which must be between 1 and 2100000000 with minor and patch below 1000"
        ));
    }
    Ok(code as u32)
}

/// The apk's `AndroidManifest.xml`: a NativeActivity app (no Java code)
/// that runs `lib/<abi>/libmain.so`, full screen, that handles rotation,
/// resizing and the keyboard itself instead of being restarted.
pub fn android_manifest(game: &Game, debuggable: bool, icon: bool) -> Result<String, String> {
    let package = android_package(&game.id)?;
    let code = android_version_code(&game.version)?;
    let label = xml(&game.name);
    let icon = if icon {
        "\n        android:icon=\"@mipmap/ic_launcher\""
    } else {
        ""
    };
    let library = ANDROID_LIBRARY.trim_start_matches("lib").trim_end_matches(".so");
    Ok(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{package}"
    android:versionCode="{code}"
    android:versionName="{version}">
    <uses-sdk android:minSdkVersion="{ANDROID_MIN_SDK}" android:targetSdkVersion="{ANDROID_TARGET_SDK}" />
    <!-- wgpu draws with Vulkan, or OpenGL ES 3 where Vulkan is missing. -->
    <uses-feature android:glEsVersion="0x00030000" android:required="true" />
    <application
        android:label="{label}"{icon}
        android:hasCode="false"
        android:extractNativeLibs="false"
        android:debuggable="{debuggable}"
        android:appCategory="game"
        android:isGame="true"
        android:enableOnBackInvokedCallback="false">
        <activity
            android:name="android.app.NativeActivity"
            android:label="{label}"
            android:exported="true"
            android:launchMode="singleTask"
            android:screenOrientation="{orientation}"
            android:resizeableActivity="true"
            android:configChanges="orientation|screenSize|smallestScreenSize|screenLayout|density|keyboard|keyboardHidden|navigation|uiMode"
            android:windowSoftInputMode="adjustNothing"
            android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen">
            <meta-data android:name="android.app.lib_name" android:value="{library}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        version = xml(&game.version),
        orientation = game.orientation.android_value(),
    ))
}

/// The third-party notices' file name.
pub const NOTICES: &str = "THIRD-PARTY-NOTICES.txt";

/// A name usable as a file name on every OS.
fn file_safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    s.trim().trim_matches('.').to_string()
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The version macOS shows: the numbers of a Cargo version, without a
/// pre-release or build part (`0.2.0-rc.1` -> `0.2.0`).
pub fn bundle_version(version: &str) -> &str {
    version.split(['-', '+']).next().unwrap_or(version)
}

/// The app bundle's `Info.plist`.
pub fn info_plist(game: &Game, platform: &Platform, icon: bool) -> String {
    let minimum = if platform.arch == "aarch64" { "11.0" } else { "10.13" };
    let mut keys = vec![
        ("CFBundleDevelopmentRegion", "<string>en</string>".to_string()),
        ("CFBundleDisplayName", format!("<string>{}</string>", xml(&game.name))),
        ("CFBundleExecutable", format!("<string>{}</string>", xml(&game.target))),
        ("CFBundleIdentifier", format!("<string>{}</string>", xml(&game.id))),
        ("CFBundleInfoDictionaryVersion", "<string>6.0</string>".to_string()),
        ("CFBundleName", format!("<string>{}</string>", xml(&game.name))),
        ("CFBundlePackageType", "<string>APPL</string>".to_string()),
        (
            "CFBundleShortVersionString",
            format!("<string>{}</string>", xml(bundle_version(&game.version))),
        ),
        (
            "CFBundleVersion",
            format!("<string>{}</string>", xml(bundle_version(&game.version))),
        ),
        (
            "LSApplicationCategoryType",
            "<string>public.app-category.games</string>".to_string(),
        ),
        ("LSMinimumSystemVersion", format!("<string>{minimum}</string>")),
        ("NSHighResolutionCapable", "<true/>".to_string()),
    ];
    if icon {
        keys.push(("CFBundleIconFile", "<string>icon</string>".to_string()));
        keys.sort();
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    for (key, value) in keys {
        out.push_str(&format!("\t<key>{key}</key>\n\t{value}\n"));
    }
    out.push_str("</dict>\n</plist>\n");
    out
}

/// A property list value, for [`ios_info_plist`].
enum Plist {
    String(String),
    Bool(bool),
    Strings(Vec<String>),
    Integers(Vec<i64>),
    Array(Vec<Plist>),
    Dict(Vec<(&'static str, Plist)>),
}

impl Plist {
    fn text(s: impl Into<String>) -> Plist {
        Plist::String(s.into())
    }
    fn write(&self, indent: usize, out: &mut String) {
        let tabs = "\t".repeat(indent);
        match self {
            Plist::String(s) => out.push_str(&format!("{tabs}<string>{}</string>\n", xml(s))),
            Plist::Bool(true) => out.push_str(&format!("{tabs}<true/>\n")),
            Plist::Bool(false) => out.push_str(&format!("{tabs}<false/>\n")),
            Plist::Strings(list) => {
                out.push_str(&format!("{tabs}<array>\n"));
                for s in list {
                    Plist::text(s.clone()).write(indent + 1, out);
                }
                out.push_str(&format!("{tabs}</array>\n"));
            }
            Plist::Integers(list) => {
                out.push_str(&format!("{tabs}<array>\n"));
                for n in list {
                    out.push_str(&format!("{tabs}\t<integer>{n}</integer>\n"));
                }
                out.push_str(&format!("{tabs}</array>\n"));
            }
            Plist::Array(list) => {
                out.push_str(&format!("{tabs}<array>\n"));
                for item in list {
                    item.write(indent + 1, out);
                }
                out.push_str(&format!("{tabs}</array>\n"));
            }
            Plist::Dict(keys) => {
                out.push_str(&format!("{tabs}<dict>\n"));
                for (key, value) in keys {
                    out.push_str(&format!("{tabs}\t<key>{key}</key>\n"));
                    value.write(indent + 1, out);
                }
                out.push_str(&format!("{tabs}</dict>\n"));
            }
        }
    }
}

/// The iPhone and iPad icon files an iOS app holds without an asset
/// catalog, by the names `CFBundleIconFiles` finds them under, and their
/// sizes in pixels.
pub const IOS_ICONS: &[(&str, u32)] = &[
    ("AppIcon60x60@2x.png", 120),
    ("AppIcon60x60@3x.png", 180),
    ("AppIcon76x76@2x~ipad.png", 152),
    ("AppIcon83.5x83.5@2x~ipad.png", 167),
];

/// An iOS app's `Info.plist`: the program, id and version, the device
/// families, Metal required, full screen with no status bar, a launch
/// screen (without one iOS shows the game letterboxed at an old phone's
/// size), one window scene (since iOS 27's SDK an app without UIKit's
/// scene life cycle isn't launched; the engine puts its window in it), the
/// orientations, and the icon files.
pub fn ios_info_plist(game: &Game, platform: &Platform, icon: bool) -> String {
    let (sdk, platform_name) = if platform.os == Os::IosSimulator {
        ("iphonesimulator", "iPhoneSimulator")
    } else {
        ("iphoneos", "iPhoneOS")
    };
    let version = bundle_version(&game.version);
    let strings = |list: Vec<&str>| Plist::Strings(list.into_iter().map(str::to_string).collect());
    let mut keys: Vec<(&'static str, Plist)> = vec![
        ("CFBundleDevelopmentRegion", Plist::text("en")),
        ("CFBundleDisplayName", Plist::text(&game.name)),
        ("CFBundleExecutable", Plist::text(&game.target)),
        ("CFBundleIdentifier", Plist::text(&game.id)),
        ("CFBundleInfoDictionaryVersion", Plist::text("6.0")),
        ("CFBundleName", Plist::text(&game.name)),
        ("CFBundlePackageType", Plist::text("APPL")),
        ("CFBundleShortVersionString", Plist::text(version)),
        ("CFBundleSupportedPlatforms", strings(vec![platform_name])),
        ("CFBundleVersion", Plist::text(version)),
        ("DTPlatformName", Plist::text(sdk)),
        ("LSApplicationCategoryType", Plist::text("public.app-category.games")),
        ("LSRequiresIPhoneOS", Plist::Bool(true)),
        ("MinimumOSVersion", Plist::text(IOS_MINIMUM)),
        (
            "UIApplicationSceneManifest",
            Plist::Dict(vec![
                ("UIApplicationSupportsMultipleScenes", Plist::Bool(false)),
                (
                    "UISceneConfigurations",
                    Plist::Dict(vec![(
                        "UIWindowSceneSessionRoleApplication",
                        Plist::Array(vec![Plist::Dict(vec![(
                            "UISceneConfigurationName",
                            Plist::text("Default"),
                        )])]),
                    )]),
                ),
            ]),
        ),
        ("UIDeviceFamily", Plist::Integers(vec![1, 2])),
        ("UILaunchScreen", Plist::Dict(Vec::new())),
        ("UIRequiredDeviceCapabilities", strings(vec!["arm64", "metal"])),
        ("UIRequiresFullScreen", Plist::Bool(true)),
        ("UIStatusBarHidden", Plist::Bool(true)),
        (
            "UISupportedInterfaceOrientations",
            strings(game.orientation.ios_values(false)),
        ),
        (
            "UISupportedInterfaceOrientations~ipad",
            strings(game.orientation.ios_values(true)),
        ),
    ];
    if icon {
        let names = |sizes: &[&str]| strings(sizes.to_vec());
        keys.push((
            "CFBundleIcons",
            Plist::Dict(vec![(
                "CFBundlePrimaryIcon",
                Plist::Dict(vec![("CFBundleIconFiles", names(&["AppIcon60x60"]))]),
            )]),
        ));
        keys.push((
            "CFBundleIcons~ipad",
            Plist::Dict(vec![(
                "CFBundlePrimaryIcon",
                Plist::Dict(vec![(
                    "CFBundleIconFiles",
                    names(&["AppIcon60x60", "AppIcon76x76", "AppIcon83.5x83.5"]),
                )]),
            )]),
        ));
        keys.sort_by_key(|(k, _)| *k);
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n",
    );
    Plist::Dict(keys).write(0, &mut out);
    out.push_str("</plist>\n");
    out
}

/// The web page that starts the game: the engine's `web/index.html`
/// (embedded; a test keeps the copies equal), titled, with the icon.
pub fn web_index(name: &str, icon: bool) -> String {
    let page = WEB_INDEX.replace("{{TITLE}}", &xml(name));
    if icon {
        page.replace(
            "<link rel=\"icon\" href=\"data:,\">",
            "<link rel=\"icon\" href=\"icon.png\">",
        )
    } else {
        page
    }
}

pub const WEB_INDEX: &str = include_str!("index.html");

/// The worker script of a web build with threads: the engine's
/// `web/ponko-worker.js` (embedded; a test keeps the copies equal).
pub const WEB_WORKER: &str = include_str!("ponko-worker.js");

/// The service worker that sends the cross-origin isolation headers on
/// hosts that can't (`--isolation-worker`): the engine's
/// `web/ponko-isolate.js` (embedded; a test keeps the copies equal).
pub const WEB_ISOLATE: &str = include_str!("ponko-isolate.js");

/// The page with the script that registers [`WEB_ISOLATE`], before the
/// game's own script (as `web/build.sh --isolation-worker`).
pub fn with_isolation_worker(page: &str) -> String {
    page.replacen("</head>", "  <script src=\"ponko-isolate.js\"></script>\n</head>", 1)
}

/// The response headers that make a page cross-origin isolated, which a
/// web build with threads needs to start its workers.
pub const ISOLATION_HEADERS: [(&str, &str); 2] = [
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Cross-Origin-Embedder-Policy", "require-corp"),
];

/// The files, beside `index.html`, that make common static hosts send
/// [`ISOLATION_HEADERS`] with every file: `_headers` (Netlify, Cloudflare
/// Pages), `vercel.json` (Vercel) and `.htaccess` (Apache with
/// mod_headers). Hosts that read none of them ignore them.
pub fn web_header_files() -> Vec<(&'static str, String)> {
    let netlify: String = ISOLATION_HEADERS.iter().map(|(k, v)| format!("  {k}: {v}\n")).collect();
    let vercel = serde_json::json!({
        "headers": [{
            "source": "/(.*)",
            "headers": ISOLATION_HEADERS
                .iter()
                .map(|(k, v)| serde_json::json!({"key": k, "value": v}))
                .collect::<Vec<_>>(),
        }]
    });
    let apache: String = ISOLATION_HEADERS
        .iter()
        .map(|(k, v)| format!("  Header set {k} \"{v}\"\n"))
        .collect();
    vec![
        ("_headers", format!("/*\n{netlify}")),
        (
            "vercel.json",
            serde_json::to_string_pretty(&vercel).unwrap_or_default() + "\n",
        ),
        (".htaccess", format!("<IfModule mod_headers.c>\n{apache}</IfModule>\n")),
    ]
}

/// The page with the game's canvas naming its asset pack (`data-pack`),
/// which the engine fetches, with HTTP range requests, before setup.
pub fn with_web_pack(page: &str) -> String {
    page.replacen(
        "<canvas id=\"ponko\" tabindex=\"0\">",
        &format!(
            "<canvas id=\"ponko\" tabindex=\"0\" data-pack=\"{}\">",
            crate::pack::DEFAULT_PACK
        ),
        1,
    )
}

/// `assets/index.json`: the files under `assets/` the engine fetches before
/// setup in a browser, relative to `assets/` and sorted (as `web/build.sh`).
pub fn web_asset_index(assets: &[FileCopy]) -> Option<String> {
    let mut names: Vec<String> = assets
        .iter()
        .filter_map(|a| a.to.strip_prefix("assets").ok())
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|n| !n.is_empty() && n != "index.json")
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort();
    Some(Value::from(names).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn game() -> Game {
        Game {
            name: "Coin Dash".into(),
            slug: "coin-dash".into(),
            id: "dev.example.coin-dash".into(),
            version: "1.2.3".into(),
            target: "coin_dash".into(),
            icon: None,
            assets: vec![FileCopy {
                from: "/src/assets/coin_dash/coin.png".into(),
                to: "assets/coin_dash/coin.png".into(),
            }],
            files: vec![
                FileCopy {
                    from: "/src/LICENSE-MIT".into(),
                    to: "LICENSE-MIT".into(),
                },
                FileCopy {
                    from: "/src/docs/PLAYER.txt".into(),
                    to: "README.txt".into(),
                },
            ],
            orientation: Orientation::Any,
            windows_dxc: true,
        }
    }

    fn platform(triple: &str) -> Platform {
        Platform::from_triple(triple).unwrap()
    }

    fn paths(plan: &Plan) -> Vec<String> {
        let mut out: Vec<String> = plan
            .entries
            .iter()
            .map(|e| e.path.to_string_lossy().replace('\\', "/"))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn windows_alias_selects_the_x64_msvc_target() {
        assert_eq!(target_alias("windows"), "x86_64-pc-windows-msvc");
    }

    #[test]
    fn windows_dxc_accepts_booleans_and_example_overrides() {
        let meta = json!({"ponko": {
            "windows-dxc": false,
            "examples": {"demo": {"windows-dxc": true}}
        }});
        assert_eq!(Metadata::from_cargo(&json!({}), None).unwrap().windows_dxc, None);
        assert_eq!(Metadata::from_cargo(&meta, None).unwrap().windows_dxc, Some(false));
        assert_eq!(
            Metadata::from_cargo(&meta, Some("demo")).unwrap().windows_dxc,
            Some(true)
        );
        let bad = json!({"ponko": {"windows-dxc": "false"}});
        assert!(Metadata::from_cargo(&bad, None).unwrap_err().contains("windows-dxc"));
    }

    #[test]
    fn triples_name_the_platform() {
        let cases = [
            ("x86_64-pc-windows-msvc", Os::Windows, "windows-x86_64"),
            ("x86_64-pc-windows-gnu", Os::Windows, "windows-x86_64"),
            ("aarch64-apple-darwin", Os::Mac, "macos-aarch64"),
            ("x86_64-unknown-linux-gnu", Os::Linux, "linux-x86_64"),
            ("wasm32-unknown-unknown", Os::Web, "web"),
            ("aarch64-apple-ios", Os::Ios, "ios-aarch64"),
            ("aarch64-apple-ios-sim", Os::IosSimulator, "ios-simulator-aarch64"),
            ("x86_64-apple-ios", Os::IosSimulator, "ios-simulator-x86_64"),
            ("aarch64-linux-android", Os::Android, "android-aarch64"),
            ("x86_64-linux-android", Os::Android, "android-x86_64"),
        ];
        for (triple, os, label) in cases {
            let p = platform(triple);
            assert_eq!((p.os, p.label().as_str()), (os, label), "{triple}");
        }
        for unsupported in [
            "riscv64-linux-android",
            "thumbv7em-none-eabihf",
            "aarch64-apple-ios-macabi",
        ] {
            assert!(Platform::from_triple(unsupported).is_err(), "{unsupported}");
        }
    }

    #[test]
    fn the_windows_package_is_a_folder_with_the_exe_assets_and_notices() {
        use super::super::dxc::Dxc;

        let dir = std::env::temp_dir().join(format!("ponko-windows-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("coin_dash.exe");
        std::fs::write(&input, b"game").unwrap();
        let mut game = game();
        for file in game.assets.iter_mut().chain(&mut game.files) {
            file.from = input.clone();
        }
        let plan = plan(
            &game,
            &platform("x86_64-pc-windows-msvc"),
            false,
            Built::Program(&input),
            "N",
            None,
            None,
        )
        .unwrap();
        let dxc = Dxc {
            dll: b"x64 compiler".to_vec(),
            license: "Fake DXC license".into(),
        };
        let plan = dxc.add_to_plan(plan).unwrap();
        assert_eq!(
            paths(&plan),
            [
                "coin-dash-1.2.3-windows-x86_64/LICENSE-MIT",
                "coin-dash-1.2.3-windows-x86_64/README.txt",
                "coin-dash-1.2.3-windows-x86_64/THIRD-PARTY-NOTICES.txt",
                "coin-dash-1.2.3-windows-x86_64/assets/coin_dash/coin.png",
                "coin-dash-1.2.3-windows-x86_64/coin_dash.exe",
                "coin-dash-1.2.3-windows-x86_64/dxcompiler.dll",
            ]
        );
        assert_eq!(plan.program, Path::new("coin-dash-1.2.3-windows-x86_64/coin_dash.exe"));
        assert!(plan.app.is_none());
        let exe = plan.entries.iter().find(|e| e.path == plan.program).unwrap();
        assert!(exe.executable);
        assert_eq!(exe.source, Source::Copy(input));

        super::super::write_plan(&dir, &plan).unwrap();
        let package = dir.join(&plan.folder);
        assert_eq!(std::fs::read(package.join("dxcompiler.dll")).unwrap(), b"x64 compiler");
        let notice = std::fs::read_to_string(package.join("THIRD-PARTY-NOTICES.txt")).unwrap();
        assert!(notice.starts_with("N\n"), "{notice}");
        assert!(notice.contains("dxcompiler.dll"), "{notice}");
        assert!(notice.contains("LICENSE-LLVM.txt\nFake DXC license"), "{notice}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_linux_package_is_a_folder_with_the_icon_beside_the_program() {
        let mut g = game();
        g.icon = Some("/src/icon.png".into());
        let plan = plan(
            &g,
            &platform("x86_64-unknown-linux-gnu"),
            false,
            Built::Program(Path::new("/t/coin_dash")),
            "N",
            Some(b"png"),
            None,
        )
        .unwrap();
        let p = paths(&plan);
        assert!(
            p.contains(&"coin-dash-1.2.3-linux-x86_64/coin_dash".to_string()),
            "{p:?}"
        );
        assert!(
            p.contains(&"coin-dash-1.2.3-linux-x86_64/icon.png".to_string()),
            "{p:?}"
        );
        assert!(p.contains(&"coin-dash-1.2.3-linux-x86_64/assets/coin_dash/coin.png".to_string()));
    }

    #[test]
    fn linked_libraries_go_beside_the_program_on_every_platform() {
        let lib = |p: &Platform| p.library_name("steam_api").unwrap();
        for (triple, app, dir) in [
            ("x86_64-pc-windows-msvc", false, "coin-dash-1.2.3-windows-x86_64"),
            ("x86_64-unknown-linux-gnu", false, "coin-dash-1.2.3-linux-x86_64"),
            (
                "aarch64-apple-darwin",
                true,
                "coin-dash-1.2.3-macos-aarch64/Coin Dash.app/Contents/MacOS",
            ),
        ] {
            let platform = platform(triple);
            let name = lib(&platform);
            let plan = plan(
                &game(),
                &platform,
                app,
                Built::Program(Path::new("/t/coin_dash")),
                "N",
                None,
                None,
            )
            .unwrap();
            let source = PathBuf::from("/t/build/out").join(&name);
            let plan = with_libraries(plan, std::slice::from_ref(&source)).unwrap();
            let entry = plan
                .entries
                .iter()
                .find(|e| e.source == Source::Copy(source.clone()))
                .unwrap();
            assert_eq!(entry.path, Path::new(dir).join(&name), "{triple}");
            assert!(!entry.executable);
            assert!(
                with_libraries(plan, &[source]).is_err(),
                "the same file twice is an error"
            );
        }
        assert_eq!(
            platform("x86_64-pc-windows-msvc").library_name("steam_api64").unwrap(),
            "steam_api64.dll"
        );
        assert_eq!(
            platform("aarch64-apple-darwin").library_name("steam_api").unwrap(),
            "libsteam_api.dylib"
        );
        assert_eq!(
            platform("x86_64-unknown-linux-gnu").library_name("steam_api").unwrap(),
            "libsteam_api.so"
        );
        assert_eq!(platform("wasm32-unknown-unknown").library_name("x"), None);
    }

    #[test]
    fn the_macos_package_is_an_app_bundle_with_assets_in_resources() {
        let plan = plan(
            &game(),
            &platform("aarch64-apple-darwin"),
            true,
            Built::Program(Path::new("/t/coin_dash")),
            "N",
            None,
            None,
        )
        .unwrap();
        let app = "coin-dash-1.2.3-macos-aarch64/Coin Dash.app/Contents";
        let p = paths(&plan);
        for want in [
            format!("{app}/Info.plist"),
            format!("{app}/MacOS/coin_dash"),
            format!("{app}/Resources/assets/coin_dash/coin.png"),
            format!("{app}/Resources/LICENSE-MIT"),
            format!("{app}/Resources/THIRD-PARTY-NOTICES.txt"),
            // Readable beside the app, too.
            "coin-dash-1.2.3-macos-aarch64/README.txt".to_string(),
            "coin-dash-1.2.3-macos-aarch64/THIRD-PARTY-NOTICES.txt".to_string(),
        ] {
            assert!(p.contains(&want), "{want} missing from {p:#?}");
        }
        assert_eq!(
            plan.app.as_deref(),
            Some(Path::new("coin-dash-1.2.3-macos-aarch64/Coin Dash.app"))
        );
        assert_eq!(plan.program, Path::new(&format!("{app}/MacOS/coin_dash")));
        // No icon given: none in the bundle, and the plist names none.
        assert!(!p.iter().any(|f| f.ends_with(".icns")));
    }

    #[test]
    fn a_macos_folder_package_is_flat_like_the_others() {
        let plan = plan(
            &game(),
            &platform("aarch64-apple-darwin"),
            false,
            Built::Program(Path::new("/t/coin_dash")),
            "N",
            None,
            None,
        )
        .unwrap();
        assert!(plan.app.is_none());
        assert!(paths(&plan).contains(&"coin-dash-1.2.3-macos-aarch64/coin_dash".to_string()));
    }

    #[test]
    fn the_info_plist_names_the_program_id_version_and_icon() {
        let mut g = game();
        g.version = "0.2.0-rc.1".into();
        g.name = "A & B <Game>".into();
        let plist = info_plist(&g, &platform("aarch64-apple-darwin"), true);
        for want in [
            "<key>CFBundleExecutable</key>\n\t<string>coin_dash</string>",
            "<key>CFBundleIdentifier</key>\n\t<string>dev.example.coin-dash</string>",
            "<key>CFBundleShortVersionString</key>\n\t<string>0.2.0</string>",
            "<key>CFBundleName</key>\n\t<string>A &amp; B &lt;Game&gt;</string>",
            "<key>CFBundleIconFile</key>\n\t<string>icon</string>",
            "<key>CFBundlePackageType</key>\n\t<string>APPL</string>",
            "<key>NSHighResolutionCapable</key>\n\t<true/>",
            "<key>LSMinimumSystemVersion</key>\n\t<string>11.0</string>",
        ] {
            assert!(plist.contains(want), "{want} not in\n{plist}");
        }
        assert!(!info_plist(&g, &platform("aarch64-apple-darwin"), false).contains("CFBundleIconFile"));
        assert!(info_plist(&g, &platform("x86_64-apple-darwin"), false).contains("<string>10.13</string>"));
    }

    #[test]
    fn the_web_package_has_the_page_the_wasm_assets_and_their_index() {
        let mut g = game();
        g.assets.push(FileCopy {
            from: "/src/assets/a.json".into(),
            to: "assets/a.json".into(),
        });
        let plan = plan(
            &g,
            &platform("wasm32-unknown-unknown"),
            false,
            Built::Web {
                dir: Path::new("/bindgen"),
                threads: false,
                isolation_worker: false,
            },
            "N",
            Some(b"png"),
            None,
        )
        .unwrap();
        let p = paths(&plan);
        for want in [
            "index.html",
            "game.js",
            "game_bg.wasm",
            "assets/index.json",
            "assets/coin_dash/coin.png",
            "icon.png",
            "THIRD-PARTY-NOTICES.txt",
        ] {
            assert!(
                p.contains(&format!("coin-dash-1.2.3-web/{want}")),
                "{want} missing from {p:#?}"
            );
        }
        let text = |name: &str| match &plan.entries.iter().find(|e| e.path.ends_with(name)).unwrap().source {
            Source::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(text("index.json"), r#"["a.json","coin_dash/coin.png"]"#);
        let page = text("index.html");
        assert!(
            page.contains("<title>Coin Dash · Ponko</title>") && page.contains("href=\"icon.png\""),
            "{page}"
        );
        assert!(page.contains("import init, * as game from \"./game.js\""));
        // Without threads: no worker script and no header files.
        for extra in ["ponko-worker.js", "_headers", "vercel.json", ".htaccess"] {
            assert!(!p.iter().any(|f| f.ends_with(extra)), "{extra} in {p:#?}");
        }
    }

    #[test]
    fn a_web_package_with_threads_has_the_worker_and_headers_for_common_hosts() {
        let mut g = game();
        // The game's own vercel.json stays; the other header files are added.
        g.files.push(FileCopy {
            from: "/src/vercel.json".into(),
            to: "vercel.json".into(),
        });
        let plan = plan(
            &g,
            &platform("wasm32-unknown-unknown"),
            false,
            Built::Web {
                dir: Path::new("/bindgen"),
                threads: true,
                isolation_worker: false,
            },
            "N",
            None,
            None,
        )
        .unwrap();
        let entry = |name: &str| {
            plan.entries
                .iter()
                .find(|e| e.path == Path::new("coin-dash-1.2.3-web").join(name))
                .unwrap_or_else(|| panic!("{name} missing from {:#?}", paths(&plan)))
        };
        let text = |name: &str| match &entry(name).source {
            Source::Text(t) => t.clone(),
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(text("ponko-worker.js"), WEB_WORKER);
        assert_eq!(entry("vercel.json").source, Source::Copy("/src/vercel.json".into()));
        for name in ["_headers", ".htaccess"] {
            let file = text(name);
            for (header, value) in ISOLATION_HEADERS {
                assert!(file.contains(header) && file.contains(value), "{name}:\n{file}");
            }
        }
        assert!(text("_headers").starts_with("/*\n"), "Netlify's rule for every path");
        let vercel: Value = serde_json::from_str(&web_header_files()[1].1).unwrap();
        assert_eq!(vercel["headers"][0]["source"], "/(.*)");
        assert_eq!(
            vercel["headers"][0]["headers"][1],
            json!({"key": "Cross-Origin-Embedder-Policy", "value": "require-corp"})
        );
    }

    #[test]
    fn a_packed_package_holds_assets_pack_where_the_assets_folder_would_be() {
        let mut g = game();
        // An asset outside assets/ stays a loose file beside the pack.
        g.assets.push(FileCopy {
            from: "/src/data/extra.txt".into(),
            to: "data/extra.txt".into(),
        });
        let pack = b"PONKOPAK...".as_slice();
        let cases = [
            ("x86_64-pc-windows-msvc", false, "coin-dash-1.2.3-windows-x86_64"),
            ("x86_64-unknown-linux-gnu", false, "coin-dash-1.2.3-linux-x86_64"),
            (
                "aarch64-apple-darwin",
                true,
                "coin-dash-1.2.3-macos-aarch64/Coin Dash.app/Contents/Resources",
            ),
        ];
        for (triple, app, dir) in cases {
            let plan = plan(
                &g,
                &platform(triple),
                app,
                Built::Program(Path::new("/t/coin_dash")),
                "N",
                None,
                Some(pack),
            )
            .unwrap();
            let p = paths(&plan);
            assert!(p.contains(&format!("{dir}/assets.pack")), "{triple}: {p:#?}");
            assert!(p.contains(&format!("{dir}/data/extra.txt")), "{triple}: {p:#?}");
            assert!(
                !p.iter().any(|f| f.contains("/assets/")),
                "{triple}: no loose assets/ {p:#?}"
            );
            let packed = plan.entries.iter().find(|e| e.path.ends_with("assets.pack")).unwrap();
            assert_eq!(packed.source, Source::Bytes(pack.to_vec()));
        }
        let web = plan(
            &g,
            &platform("wasm32-unknown-unknown"),
            false,
            Built::Web {
                dir: Path::new("/bindgen"),
                threads: false,
                isolation_worker: false,
            },
            "N",
            None,
            Some(pack),
        )
        .unwrap();
        let p = paths(&web);
        assert!(p.contains(&"coin-dash-1.2.3-web/assets.pack".to_string()), "{p:#?}");
        assert!(
            !p.iter().any(|f| f.ends_with("index.json")),
            "no index of loose assets: {p:#?}"
        );
        let page = match &web
            .entries
            .iter()
            .find(|e| e.path.ends_with("index.html"))
            .unwrap()
            .source
        {
            Source::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        assert!(
            page.contains(r#"<canvas id="ponko" tabindex="0" data-pack="assets.pack">"#),
            "{page}"
        );
    }

    #[test]
    fn the_pack_key_turns_packing_off_or_sets_it_up() {
        let m = Metadata::from_cargo(&json!({"ponko": {"pack": false}}), None).unwrap();
        assert!(!m.pack.unwrap().enabled);
        let meta = json!({"ponko": {
            "pack": {"exclude": ["**/*.psd"], "textures": true, "cook": false},
            "examples": {"demo": {"pack": false}}
        }});
        let m = Metadata::from_cargo(&meta, None).unwrap();
        assert_eq!(
            m.pack.unwrap(),
            PackMeta {
                enabled: true,
                exclude: vec!["**/*.psd".into()],
                textures: true,
                cook: false
            }
        );
        assert!(PackMeta::default().cook, "packages are cooked unless the game says no");
        assert!(!Metadata::from_cargo(&meta, Some("demo")).unwrap().pack.unwrap().enabled);
        assert_eq!(Metadata::from_cargo(&json!(null), None).unwrap().pack, None);
        for (bad, needle) in [
            (
                json!({"ponko": {"pack": "yes"}}),
                "pack: expected true, false or a table",
            ),
            (json!({"ponko": {"pack": {"level": 9}}}), "pack: unknown key `level`"),
            (
                json!({"ponko": {"pack": {"exclude": "x"}}}),
                "pack.exclude: expected a list",
            ),
        ] {
            let err = Metadata::from_cargo(&bad, None).unwrap_err();
            assert!(err.contains(needle), "{err} (wanted {needle})");
        }
    }

    #[test]
    fn the_embedded_web_page_is_the_engines() {
        // The crate is published alone, so it carries a copy of web/index.html.
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/index.html");
        if let Ok(text) = std::fs::read_to_string(engine) {
            assert_eq!(
                WEB_INDEX, text,
                "copy web/index.html to crates/ponko_cli/src/package/index.html"
            );
        }
    }

    #[test]
    fn the_embedded_isolation_worker_is_the_engines() {
        // As index.html: the published crate carries a copy of web/ponko-isolate.js.
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/ponko-isolate.js");
        if let Ok(text) = std::fs::read_to_string(engine) {
            assert_eq!(
                WEB_ISOLATE, text,
                "copy web/ponko-isolate.js to crates/ponko_cli/src/package/ponko-isolate.js"
            );
        }
    }

    #[test]
    fn an_isolation_worker_package_has_the_script_and_loads_it_before_the_game() {
        let plan = plan(
            &game(),
            &platform("wasm32-unknown-unknown"),
            false,
            Built::Web {
                dir: Path::new("/bindgen"),
                threads: true,
                isolation_worker: true,
            },
            "N",
            None,
            None,
        )
        .unwrap();
        let text = |name: &str| match &plan
            .entries
            .iter()
            .find(|e| e.path == Path::new("coin-dash-1.2.3-web").join(name))
            .unwrap_or_else(|| panic!("{name} missing from {:#?}", paths(&plan)))
            .source
        {
            Source::Text(t) => t.clone(),
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(text("ponko-isolate.js"), WEB_ISOLATE);
        let page = text("index.html");
        let script = page
            .find("<script src=\"ponko-isolate.js\"></script>")
            .expect("the page loads the worker");
        let game = page.find("import init").expect("the game's script");
        assert!(script < page.find("</head>").unwrap() && script < game, "{page}");
    }

    #[test]
    fn the_embedded_worker_script_is_the_engines() {
        // As index.html: the published crate carries a copy of web/ponko-worker.js.
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/ponko-worker.js");
        if let Ok(text) = std::fs::read_to_string(engine) {
            assert_eq!(
                WEB_WORKER, text,
                "copy web/ponko-worker.js to crates/ponko_cli/src/package/ponko-worker.js"
            );
        }
    }

    #[test]
    fn two_files_at_one_place_are_refused() {
        let mut g = game();
        g.files.push(FileCopy {
            from: "/elsewhere/LICENSE-MIT".into(),
            to: "LICENSE-MIT".into(),
        });
        let err = plan(
            &g,
            &platform("x86_64-unknown-linux-gnu"),
            false,
            Built::Program(Path::new("/t/x")),
            "",
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("LICENSE-MIT"), "{err}");
    }

    #[test]
    fn files_can_go_into_subfolders_of_the_package() {
        let meta = json!({"ponko": {"files": [
            {"from": "docs/r.txt", "to": "docs/README.txt"},
            {"from": "replays/battle.rec", "to": "replays/demo/battle.rec"},
        ]}});
        let m = Metadata::from_cargo(&meta, None).unwrap();
        assert_eq!(
            m.files.unwrap(),
            [
                ("docs/r.txt".to_string(), "docs/README.txt".to_string()),
                ("replays/battle.rec".into(), "replays/demo/battle.rec".into())
            ]
        );
        let mut g = game();
        g.files.push(FileCopy {
            from: "/src/docs/manual.pdf".into(),
            to: "docs/manual.pdf".into(),
        });
        let linux = plan(
            &g,
            &platform("x86_64-unknown-linux-gnu"),
            false,
            Built::Program(Path::new("/t/coin_dash")),
            "",
            None,
            None,
        )
        .unwrap();
        let folder = "coin-dash-1.2.3-linux-x86_64";
        assert!(
            paths(&linux).contains(&format!("{folder}/docs/manual.pdf")),
            "{:?}",
            paths(&linux)
        );
        // A macOS app has its files inside the bundle and beside it.
        let mac = plan(
            &g,
            &platform("aarch64-apple-darwin"),
            true,
            Built::Program(Path::new("/t/coin_dash")),
            "",
            None,
            None,
        )
        .unwrap();
        let mac_paths = paths(&mac);
        let folder = "coin-dash-1.2.3-macos-aarch64";
        for at in [
            format!("{folder}/docs/manual.pdf"),
            format!("{folder}/Coin Dash.app/Contents/Resources/docs/manual.pdf"),
        ] {
            assert!(mac_paths.contains(&at), "{at} in {mac_paths:?}");
        }
    }

    #[test]
    fn a_file_where_another_file_needs_a_folder_is_refused() {
        let mut g = game();
        g.files.push(FileCopy {
            from: "/src/docs.txt".into(),
            to: "README.txt/extra.txt".into(),
        });
        let err = plan(
            &g,
            &platform("x86_64-unknown-linux-gnu"),
            false,
            Built::Program(Path::new("/t/x")),
            "",
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.contains("README.txt") && err.contains("a file and the folder"),
            "{err}"
        );
    }

    #[test]
    fn formats_default_per_platform_and_are_checked() {
        assert_eq!(Format::defaults(Os::Windows, true), [Format::Zip]);
        assert_eq!(Format::defaults(Os::Linux, true), [Format::TarGz]);
        assert_eq!(Format::defaults(Os::Mac, true), [Format::Dmg]);
        assert_eq!(Format::defaults(Os::Mac, false), [Format::Zip]);
        assert_eq!(Format::parse_list("zip, tar.gz").unwrap(), [Format::Zip, Format::TarGz]);
        assert!(Format::parse_list("rar").unwrap_err().contains("rar"));
        let mac = platform("aarch64-apple-darwin");
        assert_eq!(check_formats(&mac, &[Format::Dmg]).unwrap(), (true, vec![Format::Dmg]));
        assert_eq!(check_formats(&mac, &[Format::Folder]).unwrap(), (false, vec![]));
        assert!(check_formats(&platform("x86_64-pc-windows-msvc"), &[Format::Dmg]).is_err());
        assert!(check_formats(&mac, &[Format::Folder, Format::Dmg]).is_err());
    }

    #[test]
    fn ios_packages_are_app_bundles_in_an_ipa_or_a_zip_for_the_simulator() {
        assert_eq!(Format::defaults(Os::Ios, true), [Format::Ipa]);
        assert_eq!(Format::defaults(Os::IosSimulator, true), [Format::Zip]);
        assert_eq!(Format::parse_list("ipa").unwrap(), [Format::Ipa]);
        let device = platform("aarch64-apple-ios");
        let simulator = platform("aarch64-apple-ios-sim");
        assert_eq!(
            check_formats(&device, &[Format::Ipa]).unwrap(),
            (true, vec![Format::Ipa])
        );
        // `folder` keeps the folder: an iOS app is still a bundle in it.
        assert_eq!(check_formats(&simulator, &[Format::Folder]).unwrap(), (true, vec![]));
        assert!(
            check_formats(&simulator, &[Format::Ipa])
                .unwrap_err()
                .contains("simulator")
        );
        assert!(check_formats(&platform("aarch64-apple-darwin"), &[Format::Ipa]).is_err());
        assert!(check_formats(&device, &[Format::TarGz]).is_err());
        assert!(check_formats(&device, &[Format::Dmg]).is_err());
    }

    #[test]
    fn the_ios_package_is_a_flat_app_bundle_with_the_program_assets_and_notices() {
        let mut icon = Vec::new();
        image::RgbaImage::from_pixel(512, 512, image::Rgba([0, 0, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut icon), image::ImageFormat::Png)
            .unwrap();
        let plan = plan(
            &game(),
            &platform("aarch64-apple-ios-sim"),
            true,
            Built::Program(Path::new("/t/coin_dash")),
            "N",
            Some(&icon),
            Some(b"PACK"),
        )
        .unwrap();
        let app = "coin-dash-1.2.3-ios-simulator-aarch64/Coin Dash.app";
        let p = paths(&plan);
        for want in [
            format!("{app}/Info.plist"),
            format!("{app}/coin_dash"),
            format!("{app}/assets.pack"),
            format!("{app}/AppIcon60x60@2x.png"),
            format!("{app}/AppIcon83.5x83.5@2x~ipad.png"),
            format!("{app}/LICENSE-MIT"),
            format!("{app}/THIRD-PARTY-NOTICES.txt"),
            "coin-dash-1.2.3-ios-simulator-aarch64/THIRD-PARTY-NOTICES.txt".to_string(),
        ] {
            assert!(p.contains(&want), "{want} missing from {p:#?}");
        }
        assert_eq!(plan.app.as_deref(), Some(Path::new(app)));
        assert_eq!(plan.program, Path::new(&format!("{app}/coin_dash")));
        assert!(plan.entries.iter().any(|e| e.executable && e.path == plan.program));
        // The program finds the pack beside itself, as on Windows and Linux.
        assert!(!p.iter().any(|f| f.contains("/Contents/")));
    }

    #[test]
    fn the_ios_info_plist_asks_for_metal_full_screen_a_launch_screen_and_the_orientations() {
        let mut g = game();
        g.orientation = Orientation::Landscape;
        let sim = ios_info_plist(&g, &platform("aarch64-apple-ios-sim"), false);
        for want in [
            "<key>CFBundleExecutable</key>\n\t<string>coin_dash</string>",
            "<key>CFBundleIdentifier</key>\n\t<string>dev.example.coin-dash</string>",
            "<key>CFBundleSupportedPlatforms</key>\n\t<array>\n\t\t<string>iPhoneSimulator</string>",
            &format!("<key>MinimumOSVersion</key>\n\t<string>{IOS_MINIMUM}</string>"),
            "<key>UIDeviceFamily</key>\n\t<array>\n\t\t<integer>1</integer>\n\t\t<integer>2</integer>",
            "<key>UILaunchScreen</key>\n\t<dict>\n\t</dict>",
            "<key>UIApplicationSupportsMultipleScenes</key>\n\t\t<false/>",
            "<key>UIWindowSceneSessionRoleApplication</key>\n\t\t\t<array>\n\t\t\t\t<dict>\n\t\t\t\t\t<key>UISceneConfigurationName</key>",
            "<string>metal</string>",
            "<key>UIStatusBarHidden</key>\n\t<true/>",
            "<key>UISupportedInterfaceOrientations</key>\n\t<array>\n\t\t<string>UIInterfaceOrientationLandscapeLeft</string>\n\t\t<string>UIInterfaceOrientationLandscapeRight</string>\n\t</array>",
        ] {
            assert!(sim.contains(want), "{want} missing from\n{sim}");
        }
        assert!(!sim.contains("Portrait"));
        assert!(!sim.contains("CFBundleIcons"));
        let device = ios_info_plist(&game(), &platform("aarch64-apple-ios"), true);
        assert!(device.contains("<string>iPhoneOS</string>"));
        assert!(device.contains("<string>iphoneos</string>"));
        assert!(
            device.contains("<string>UIInterfaceOrientationPortraitUpsideDown</string>"),
            "iPads, any way up"
        );
        assert!(device.contains("<key>CFBundleIcons~ipad</key>"));
        assert!(device.contains("<string>AppIcon83.5x83.5</string>"));
    }

    #[test]
    fn android_packages_are_apks_app_bundles_or_the_folder_they_are_made_of() {
        assert_eq!(target_alias("android"), "aarch64-linux-android");
        assert_eq!(Format::defaults(Os::Android, true), [Format::Apk]);
        assert_eq!(Format::parse_list("apk").unwrap(), [Format::Apk]);
        let android = platform("aarch64-linux-android");
        assert_eq!(
            check_formats(&android, &[Format::Apk]).unwrap(),
            (true, vec![Format::Apk])
        );
        assert_eq!(check_formats(&android, &[Format::Folder]).unwrap(), (true, vec![]));
        assert_eq!(Format::parse_list("apk,aab").unwrap(), [Format::Apk, Format::Aab]);
        assert_eq!(
            check_formats(&android, &[Format::Apk, Format::Aab]).unwrap(),
            (true, vec![Format::Apk, Format::Aab])
        );
        assert_eq!(Format::Aab.extension(), "aab");
        assert!(
            check_formats(&platform("aarch64-apple-ios"), &[Format::Aab])
                .unwrap_err()
                .contains("an aab is for Android")
        );
        for other in [Format::Zip, Format::TarGz, Format::Dmg, Format::Ipa] {
            assert!(check_formats(&android, &[other]).is_err(), "{other:?}");
        }
        assert!(check_formats(&platform("x86_64-unknown-linux-gnu"), &[Format::Apk]).is_err());
        assert_eq!(android.library_name("steam_api"), Some("libsteam_api.so".into()));
    }

    #[test]
    fn the_android_package_holds_the_library_pack_notices_and_icons_for_the_apk() {
        let mut icon = Vec::new();
        image::RgbaImage::from_pixel(512, 512, image::Rgba([0, 0, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut icon), image::ImageFormat::Png)
            .unwrap();
        let built = Built::Android {
            library: Path::new("/t/coin_dash"),
            debuggable: false,
        };
        let plan = plan(
            &game(),
            &platform("aarch64-linux-android"),
            true,
            built,
            "N",
            Some(&icon),
            Some(b"PACK"),
        )
        .unwrap();
        let folder = "coin-dash-1.2.3-android-aarch64";
        let apk = format!("{folder}/apk");
        let p = paths(&plan);
        for want in [
            format!("{apk}/AndroidManifest.xml"),
            format!("{apk}/lib/arm64-v8a/libmain.so"),
            format!("{apk}/assets/assets.pack"),
            format!("{apk}/assets/THIRD-PARTY-NOTICES.txt"),
            format!("{apk}/assets/LICENSE-MIT"),
            format!("{apk}/res/mipmap-mdpi/ic_launcher.png"),
            format!("{apk}/res/mipmap-xxxhdpi/ic_launcher.png"),
            format!("{folder}/THIRD-PARTY-NOTICES.txt"),
        ] {
            assert!(p.contains(&want), "{want} missing from {p:#?}");
        }
        assert_eq!(plan.app.as_deref(), Some(Path::new(&apk)));
        assert_eq!(plan.program, Path::new(&format!("{apk}/lib/arm64-v8a/libmain.so")));
        let manifest = plan
            .entries
            .iter()
            .find(|e| e.path.ends_with("AndroidManifest.xml"))
            .unwrap();
        let Source::Text(manifest) = &manifest.source else {
            panic!("the manifest is text");
        };
        assert!(manifest.contains(r#"android:icon="@mipmap/ic_launcher""#));
        assert!(manifest.contains(r#"android:debuggable="false""#));
        // Libraries a build script linked load from beside the program.
        let plan = with_libraries(plan, &[PathBuf::from("/o/libsteam_api.so")]).unwrap();
        assert!(paths(&plan).contains(&format!("{apk}/lib/arm64-v8a/libsteam_api.so")));

        // A game reads only the pack inside an apk: loose files are refused.
        let built = Built::Android {
            library: Path::new("/t/coin_dash"),
            debuggable: true,
        };
        let loose = plan_err(built);
        assert!(loose.contains("assets.pack"), "{loose}");
    }

    fn plan_err(built: Built) -> String {
        plan(
            &game(),
            &platform("aarch64-linux-android"),
            true,
            built,
            "N",
            None,
            None,
        )
        .unwrap_err()
    }

    #[test]
    fn the_android_manifest_runs_the_library_full_screen_and_handles_rotation_itself() {
        let mut g = game();
        g.name = "Coin & Dash".into();
        g.orientation = Orientation::Landscape;
        let m = android_manifest(&g, true, false).unwrap();
        for want in [
            r#"package="dev.example.coin_dash""#,
            r#"android:versionCode="1002003""#,
            r#"android:versionName="1.2.3""#,
            &format!(r#"android:minSdkVersion="{ANDROID_MIN_SDK}" android:targetSdkVersion="{ANDROID_TARGET_SDK}""#),
            r#"android:label="Coin &amp; Dash""#,
            r#"android:hasCode="false""#,
            r#"android:extractNativeLibs="false""#,
            r#"android:debuggable="true""#,
            r#"android:appCategory="game""#,
            r#"android:name="android.app.NativeActivity""#,
            r#"android:screenOrientation="userLandscape""#,
            "orientation|screenSize|smallestScreenSize|screenLayout|density|keyboard|keyboardHidden",
            r#"android:windowSoftInputMode="adjustNothing""#,
            r#"<meta-data android:name="android.app.lib_name" android:value="main" />"#,
            "android.intent.category.LAUNCHER",
        ] {
            assert!(m.contains(want), "{want} missing from\n{m}");
        }
        assert!(!m.contains("android:icon"));
        g.orientation = Orientation::Any;
        assert!(android_manifest(&g, false, true).unwrap().contains(r#""fullUser""#));
        g.orientation = Orientation::Portrait;
        assert!(android_manifest(&g, false, true).unwrap().contains(r#""userPortrait""#));
    }

    #[test]
    fn android_version_codes_grow_with_the_version_and_ids_become_package_names() {
        assert_eq!(android_version_code("1.2.3").unwrap(), 1_002_003);
        assert_eq!(android_version_code("0.1.0").unwrap(), 1_000);
        assert_eq!(android_version_code("0.2.0-rc.1").unwrap(), 2_000);
        assert!(android_version_code("1.2.3").unwrap() < android_version_code("1.10.0").unwrap());
        for bad in ["0.0.0", "1.2", "1.1000.0", "2101.0.0", "x.y.z"] {
            assert!(android_version_code(bad).is_err(), "{bad}");
        }
        assert_eq!(
            android_package("dev.example.coin-dash").unwrap(),
            "dev.example.coin_dash"
        );
        assert_eq!(android_package("com.example.game2").unwrap(), "com.example.game2");
        for bad in ["com.2fast.game", "com.example.-game"] {
            assert!(android_package(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_orientation_key_is_read_and_checked() {
        let meta = |v: serde_json::Value| Metadata::from_cargo(&json!({ "ponko": v }), None);
        assert_eq!(
            meta(json!({"orientation": "portrait"})).unwrap().orientation,
            Some(Orientation::Portrait)
        );
        assert_eq!(meta(json!({})).unwrap().orientation, None);
        assert!(
            meta(json!({"orientation": "sideways"}))
                .unwrap_err()
                .contains("orientation")
        );
    }

    #[test]
    fn metadata_reads_the_ponko_table_and_an_examples_overrides() {
        let meta = json!({"ponko": {
            "name": "My Game", "id": "com.example.my-game", "icon": "icon.png",
            "assets": ["assets"], "files": ["LICENSE", {"from": "docs/r.txt", "to": "README.txt"}],
            "examples": {"demo": {"name": "Demo", "assets": ["assets/demo"]}}
        }});
        let m = Metadata::from_cargo(&meta, None).unwrap();
        assert_eq!(m.name.as_deref(), Some("My Game"));
        assert_eq!(
            m.files.unwrap(),
            [
                ("LICENSE".to_string(), "LICENSE".to_string()),
                ("docs/r.txt".into(), "README.txt".into())
            ]
        );
        let d = Metadata::from_cargo(&meta, Some("demo")).unwrap();
        assert_eq!(d.name.as_deref(), Some("Demo"));
        assert_eq!(d.assets.unwrap(), ["assets/demo"]);
        assert_eq!(d.id.as_deref(), Some("com.example.my-game"));
        assert_eq!(Metadata::from_cargo(&json!(null), None).unwrap(), Metadata::default());
    }

    #[test]
    fn bad_metadata_is_refused_naming_the_key() {
        for (meta, needle) in [
            (json!({"ponko": {"nmae": "x"}}), "unknown key `nmae`"),
            (json!({"ponko": {"id": "mygame"}}), "reverse-DNS"),
            (json!({"ponko": {"name": 3}}), "name: expected"),
            (json!({"ponko": {"assets": "assets"}}), "assets: expected a list"),
            (json!({"ponko": {"files": [{"from": "a"}]}}), "`from` and `to`"),
            (
                json!({"ponko": {"files": [{"from": "a", "to": "../a"}]}}),
                "inside the package",
            ),
            (
                json!({"ponko": {"files": [{"from": "a", "to": "/a"}]}}),
                "inside the package",
            ),
            (
                json!({"ponko": {"files": [{"from": "a", "to": "d//a"}]}}),
                "inside the package",
            ),
            (
                json!({"ponko": {"files": [{"from": "a", "to": "d/./a"}]}}),
                "inside the package",
            ),
            (json!({"ponko": {"files": [{"from": "a", "to": "d\\a"}]}}), "with `/`"),
            (json!({"ponko": {"files": [{"from": "a", "to": "C:/a"}]}}), "`:`"),
        ] {
            let err = Metadata::from_cargo(&meta, None).unwrap_err();
            assert!(err.contains(needle), "{err} (wanted {needle})");
        }
    }

    #[test]
    fn names_become_slugs_and_paths_stay_inside() {
        assert_eq!(slug("Coin Dash"), "coin-dash");
        assert_eq!(slug("  Hello, World!! 2 "), "hello-world-2");
        assert_eq!(package_path("./assets/x").unwrap(), Path::new("assets/x"));
        for bad in ["../x", "/abs", ""] {
            assert!(package_path(bad).is_err(), "{bad}");
        }
        assert_eq!(bundle_version("1.0.0+build.5"), "1.0.0");
    }
}
