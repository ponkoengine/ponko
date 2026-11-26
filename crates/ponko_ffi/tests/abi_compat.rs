//! Native ABI compatibility against a retained baseline.
//!
//! `tests/fixtures/abi/` holds what ABI version 2 looked like at a recorded
//! engine revision (see its README and `provenance.json`): the generated
//! header, and a layout file measured by a C compiler reading that header
//! (struct sizes, alignments and field offsets, enum constants, `#define`
//! values). Nothing here asks the code under test what it used to look like.
//!
//! STABILITY.md: within an ABI version existing functions and struct layouts
//! never change, and releases only add functions and enum values. Three
//! checks follow from that:
//!
//! 1. the Rust types still have the layout the C compiler measured;
//! 2. `include/ponko.h` still declares everything the baseline header did,
//!    the same way (new functions, types and enum constants are fine);
//! 3. the library still reports ABI version 2, the number a program checks at
//!    startup.
//!
//! The old-header client check, which compiles a C program against the
//! baseline header and links the current library, is `examples/cpp/abi_baseline.c`
//! (run by the native CI job).
//!
//! Never regenerate the baseline to make a test pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ponko_ffi::*;
use serde_json::Value;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/abi")
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("can't read {}: {e}", path.display()))
}

fn layout_fixture() -> Value {
    serde_json::from_str(&read(fixture_dir().join("abi_v2_layout.json"))).expect("abi_v2_layout.json is JSON")
}

// ------------------------------------------------- Rust layouts vs the C ones

// Defines `baseline_table!`, which calls `s!`, `e!` and `c!` below.
include!("fixtures/abi/layout_table.rs");

type Offsets = Vec<(&'static str, usize)>;

#[derive(Default)]
struct RustFacts {
    structs: BTreeMap<&'static str, (usize, usize, Offsets)>,
    enums: BTreeMap<&'static str, Vec<(&'static str, i64)>>,
    constants: BTreeMap<&'static str, u64>,
}

/// What the Rust types say, read through the generated table
/// (`fixtures/abi/layout_table.rs`; it names types, it holds no numbers).
fn rust_facts() -> RustFacts {
    use std::mem::{align_of, offset_of, size_of};
    let mut l = RustFacts::default();
    macro_rules! s {
        ($t:ident: $($f:ident),+) => {
            l.structs.insert(
                stringify!($t),
                (size_of::<$t>(), align_of::<$t>(), vec![$((stringify!($f), offset_of!($t, $f))),+]),
            );
        };
    }
    macro_rules! e {
        ($t:ident: $($k:ident),+) => {
            l.enums.insert(stringify!($t), vec![$((stringify!($k), $t::$k as i64)),+]);
        };
    }
    macro_rules! c {
        ($($k:ident),+) => { $( l.constants.insert(stringify!($k), $k as u64); )+ };
    }
    baseline_table!();
    l
}

#[cfg(target_pointer_width = "64")]
fn num(v: &Value) -> usize {
    v.as_u64().expect("a number") as usize
}

#[test]
fn baseline_pins_abi_version_2_on_a_64_bit_target() {
    let b = layout_fixture();
    assert_eq!(b["abi_version"], 2);
    assert_eq!(b["pointer_bits"], 64);
    assert!(
        b["structs"].as_object().unwrap().len() >= 100,
        "the baseline lost its structs"
    );
    assert!(b["enums"].as_object().unwrap().len() >= 20);
    assert_eq!(
        b["structs"]["PonkoChunk"]["offsets"]["columns"], 16,
        "PonkoChunk.columns sits after two size_t fields"
    );
}

// The baseline was measured on a 64-bit desktop target (macOS, Linux and Windows,
// x86-64 and arm64, agree on these types). On a 32-bit target the test is not
// compiled, so it is reported as absent rather than as a pass.
#[cfg(target_pointer_width = "64")]
#[test]
fn struct_size_alignment_and_field_offsets_match_the_baseline() {
    let baseline = layout_fixture();
    let rust = rust_facts();
    let mut problems = Vec::new();
    for (name, want) in baseline["structs"].as_object().unwrap() {
        let Some((size, align, offsets)) = rust.structs.get(name.as_str()) else {
            problems.push(format!("{name}: not in the generated Rust table"));
            continue;
        };
        if *size != num(&want["size"]) {
            problems.push(format!("{name}: size is {size}, baseline {}", want["size"]));
        }
        if *align != num(&want["align"]) {
            problems.push(format!("{name}: alignment is {align}, baseline {}", want["align"]));
        }
        // cbindgen appends `_` to field names that are C++ keywords (`delete_`).
        let want_offsets: BTreeMap<&str, &Value> = want["offsets"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.trim_end_matches('_'), v))
            .collect();
        if want_offsets.len() != offsets.len() {
            problems.push(format!("{name}: field count differs from the baseline"));
        }
        for (field, offset) in offsets {
            match want_offsets.get(*field) {
                Some(w) if num(w) == *offset => {}
                Some(w) => problems.push(format!("{name}.{field}: offset is {offset}, baseline {w}")),
                None => problems.push(format!("{name}.{field}: not in the baseline")),
            }
        }
    }
    assert!(
        problems.is_empty(),
        "ABI layout differs from the retained version-2 baseline (STABILITY.md: struct layouts never \
         change within an ABI version):\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn every_baseline_struct_is_in_the_rust_table_and_back() {
    let baseline = layout_fixture();
    let rust = rust_facts();
    let in_fixture: Vec<&str> = baseline["structs"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let in_table: Vec<&str> = rust.structs.keys().copied().collect();
    for n in &in_fixture {
        assert!(
            in_table.contains(n),
            "{n} is in the baseline but not in layout_table.rs"
        );
    }
    for n in &in_table {
        assert!(
            in_fixture.contains(n),
            "{n} is in layout_table.rs but not in the baseline"
        );
    }
}

#[test]
fn enum_constants_and_defines_keep_their_baseline_values() {
    let baseline = layout_fixture();
    let rust = rust_facts();
    let mut problems = Vec::new();
    for (name, consts) in baseline["enums"].as_object().unwrap() {
        let Some(now) = rust.enums.get(name.as_str()) else {
            problems.push(format!("{name}: not in the generated Rust table"));
            continue;
        };
        for (k, want) in consts.as_object().unwrap() {
            match now.iter().find(|(n, _)| n == k) {
                Some((_, v)) if Some(*v) == want.as_i64() => {}
                Some((_, v)) => problems.push(format!("{name}::{k} is {v}, baseline {want}")),
                None => problems.push(format!("{name}::{k} is missing")),
            }
        }
    }
    for (k, want) in baseline["constants"].as_object().unwrap() {
        match rust.constants.get(k.as_str()) {
            Some(v) if Some(*v) == want.as_u64() => {}
            Some(v) => problems.push(format!("{k} is {v}, baseline {want}")),
            None => problems.push(format!("{k} is missing")),
        }
    }
    assert!(
        problems.is_empty(),
        "enum values or constants changed:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn startup_version_check_sees_the_baseline_abi_version() {
    // What STABILITY.md tells programs to do at startup.
    assert_eq!(
        u64::from(ponko_abi_version()),
        layout_fixture()["abi_version"].as_u64().unwrap()
    );
}

#[test]
fn header_macro_matches_the_library_and_the_baseline() {
    // `PONKO_ABI_VERSION` is what a C or C++ program compiled against the header
    // compares with `ponko_abi_version()` at startup (ponko.hpp does it).
    let header = current_header();
    let macro_value: u32 = header
        .lines()
        .find_map(|l| l.strip_prefix("#define PONKO_ABI_VERSION "))
        .expect("ponko.h defines PONKO_ABI_VERSION")
        .trim()
        .parse()
        .expect("PONKO_ABI_VERSION is a number");
    assert_eq!(macro_value, ponko_abi_version());
    assert_eq!(macro_value, PONKO_ABI_VERSION);
    assert_eq!(
        u64::from(macro_value),
        layout_fixture()["abi_version"].as_u64().unwrap()
    );
}

// ------------------------------------------------------- header comparison

#[derive(Debug, PartialEq)]
enum Item {
    /// A function prototype with parameter names removed.
    Function(String),
    /// A struct definition, whitespace-normalized, field names included.
    Struct(String),
    /// Constants of an enum, in order, with numeric values where known.
    Enum(Vec<(String, Option<i64>, String)>),
    /// Any other typedef.
    Typedef(String),
    Define(String),
}

fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = &rest[i + 2..];
        rest = &rest[rest.find("*/").expect("unterminated comment") + 2..];
    }
    out.push_str(rest);
    out.lines()
        .map(|l| l.split("//").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Drops a parameter's name: `const char *name` -> `const char *`.
fn strip_param_name(p: &str) -> String {
    let p = norm(p);
    if p.contains('(') || p == "void" || p == "..." {
        return p;
    }
    let tokens: Vec<&str> = p.split([' ', '*']).filter(|t| !t.is_empty()).collect();
    let type_words = ["struct", "enum", "const", "unsigned", "signed", "long", "short"];
    match tokens.as_slice() {
        [.., prev, last] if !type_words.contains(prev) && !type_words.contains(last) => {
            p[..p.len() - last.len()].trim_end().to_string()
        }
        _ => p,
    }
}

fn parse_enum_constants(body: &str) -> Vec<(String, Option<i64>, String)> {
    let mut next = Some(0i64);
    let mut out = Vec::new();
    for c in body.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        let (name, text) = match c.split_once('=') {
            Some((n, v)) => (n.trim(), Some(v.trim())),
            None => (c, None),
        };
        let value = match text {
            Some(t) => t.parse::<i64>().ok(),
            None => next,
        };
        next = value.map(|v| v + 1);
        out.push((name.to_string(), value, text.unwrap_or("").to_string()));
    }
    out
}

/// Splits a cbindgen-generated header into the items that make up the ABI.
/// Keys: `fn name`, `struct Name`, `enum Name`, `typedef Name`, `define NAME`.
fn parse_header(text: &str) -> BTreeMap<String, Item> {
    let mut items = BTreeMap::new();
    let mut code = String::new();
    for line in strip_comments(text).lines() {
        let t = line.trim();
        if let Some(def) = t.strip_prefix("#define ") {
            let mut parts = def.splitn(2, ' ');
            let name = parts.next().unwrap();
            if name != "PONKO_H" {
                items.insert(format!("define {name}"), Item::Define(norm(parts.next().unwrap_or(""))));
            }
        } else if !t.starts_with('#') && t != "}" {
            // A lone `}` closes the `extern "C" {` block (`} // extern "C"` once its
            // comment is stripped); struct and enum blocks close with `} Name;`.
            code.push_str(line);
            code.push('\n');
        }
    }
    // Statements end with `;` outside braces; `extern "C" {` is not one.
    let code = code.replace("extern \"C\" {", "");
    let (mut depth, mut start) = (0usize, 0usize);
    let mut statements = Vec::new();
    for (i, ch) in code.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                statements.push(code[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    for st in statements {
        let st = norm(&st);
        if st.is_empty() {
            continue;
        }
        let block = |s: &str| -> (String, String) {
            let open = s.find('{').unwrap();
            (
                s[..open].trim().to_string(),
                s[open + 1..s.rfind('}').unwrap()].trim().to_string(),
            )
        };
        if st.starts_with("typedef struct ") && st.contains('{') {
            let (head, _) = block(&st);
            let name = head.trim_start_matches("typedef struct ").to_string();
            items.insert(format!("struct {name}"), Item::Struct(st));
        } else if (st.starts_with("typedef enum ") || st.starts_with("enum ")) && st.contains('{') {
            let (head, body) = block(&st);
            let name = head
                .trim_start_matches("typedef ")
                .trim_start_matches("enum ")
                .to_string();
            items.insert(format!("enum {name}"), Item::Enum(parse_enum_constants(&body)));
        } else if st.starts_with("typedef ") {
            let name = match st.find("(*") {
                Some(i) => st[i + 2..].split(')').next().unwrap().to_string(),
                None => st.trim_end().rsplit(' ').next().unwrap().to_string(),
            };
            items.insert(format!("typedef {name}"), Item::Typedef(st));
        } else if let Some(open) = st.find('(') {
            let close = st.rfind(')').unwrap();
            let head = &st[..open];
            let name = head.rsplit([' ', '*']).next().unwrap().to_string();
            let ret = head[..head.len() - name.len()].trim().to_string();
            let args = &st[open + 1..close];
            // Split parameters at commas outside parentheses.
            let (mut d, mut s, mut params) = (0, 0, Vec::new());
            for (i, ch) in args.char_indices() {
                match ch {
                    '(' => d += 1,
                    ')' => d -= 1,
                    ',' if d == 0 => {
                        params.push(strip_param_name(&args[s..i]));
                        s = i + 1;
                    }
                    _ => {}
                }
            }
            params.push(strip_param_name(&args[s..]));
            items.insert(
                format!("fn {name}"),
                Item::Function(format!("{ret} {name}({})", params.join(", "))),
            );
        }
    }
    items
}

/// What a program built against the `old` header can no longer rely on in
/// `new`. Added functions, types and enum constants are not reported.
fn header_incompatibilities(old: &str, new: &str) -> Vec<String> {
    let (old, new) = (parse_header(old), parse_header(new));
    let mut out = Vec::new();
    for (key, item) in &old {
        let Some(now) = new.get(key) else {
            out.push(format!("{key}: removed"));
            continue;
        };
        match (item, now) {
            (Item::Enum(was), Item::Enum(is)) => {
                for (name, value, text) in was {
                    match is.iter().find(|(n, _, _)| n == name) {
                        None => out.push(format!("{key}: constant {name} removed")),
                        Some((_, v, t)) if v != value || (value.is_none() && t != text) => {
                            out.push(format!("{key}: constant {name} changed value"))
                        }
                        Some(_) => {}
                    }
                }
            }
            (a, b) if a == b => {}
            (a, b) => out.push(format!("{key}: changed\n      was: {a:?}\n      now: {b:?}")),
        }
    }
    out
}

fn baseline_header() -> String {
    read(fixture_dir().join("ponko_abi2.h"))
}

fn current_header() -> String {
    // The exported symbols themselves are checked through dynamic lookup
    // (scripts/test-native-symbols.py) and the baseline C client.
    read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include/ponko.h"))
}

/// The C items `scripts/experimental-api.txt` lists (`ponko_ffi::<name>`):
/// experimental, outside the promise (STABILITY.md, "Unstable"), so they may
/// change or go within an ABI version.
fn unstable_c_items() -> BTreeSet<String> {
    read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/experimental-api.txt"))
        .lines()
        .filter_map(|l| {
            l.split_whitespace()
                .next()?
                .strip_prefix("ponko_ffi::")
                .map(str::to_string)
        })
        .collect()
}

/// `header_incompatibilities` without the items in `unstable`.
fn stable_incompatibilities(old: &str, new: &str, unstable: &BTreeSet<String>) -> Vec<String> {
    header_incompatibilities(old, new)
        .into_iter()
        .filter(|m| {
            let key = m.split(':').next().unwrap_or_default();
            !key.split_once(' ').is_some_and(|(_, name)| unstable.contains(name))
        })
        .collect()
}

#[test]
fn current_header_keeps_every_baseline_declaration() {
    let found = stable_incompatibilities(&baseline_header(), &current_header(), &unstable_c_items());
    assert!(
        found.is_empty(),
        "include/ponko.h differs incompatibly from the retained ABI-2 header (STABILITY.md: existing \
         functions and structs never change within an ABI version):\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn baseline_header_parses_into_a_substantial_abi() {
    let items = parse_header(&baseline_header());
    let count = |p: &str| items.keys().filter(|k| k.starts_with(p)).count();
    assert!(count("fn ") > 500, "only {} functions parsed", count("fn "));
    assert!(count("struct ") > 100 && count("enum ") > 20 && count("define ") >= 10);
    assert_eq!(
        items["fn ponko_abi_version"],
        Item::Function("uint32_t ponko_abi_version(void)".into())
    );
    assert_eq!(
        items["fn ponko_spawn"],
        Item::Function(
            "enum PonkoStatus ponko_spawn(struct PonkoWorld *, const struct PonkoValue *, size_t, uint64_t *)".into()
        )
    );
}

const MINI: &str = r#"
#define PONKO_NONE 0
typedef struct PonkoPoint {
  float x;
  float y;
} PonkoPoint;
typedef enum PonkoMode {
  PONKO_A = 0,
  PONKO_B,
} PonkoMode;
typedef struct PonkoThing PonkoThing;
typedef void (*PonkoCallback)(struct PonkoThing *thing, void *user);
#ifdef __cplusplus
extern "C" {
#endif // __cplusplus
struct PonkoThing *ponko_thing_new(const char *name, size_t len);
enum PonkoStatus ponko_thing_move(struct PonkoThing *thing,
                                    struct PonkoPoint by);
#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus
"#;

fn changed(from: &str, to: &str) -> String {
    assert!(MINI.contains(from), "test setup: {from:?} not in MINI");
    MINI.replace(from, to)
}

fn expect_one(found: Vec<String>, fragment: &str) {
    assert!(
        found.iter().any(|m| m.contains(fragment)),
        "expected a failure mentioning {fragment:?}, got {found:?}"
    );
}

#[test]
fn items_after_the_extern_c_block_are_still_parsed() {
    let items = parse_header(&format!(
        "{MINI}\ntypedef struct PonkoAfter {{ int a; }} PonkoAfter;\nint ponko_after(void);\n"
    ));
    assert!(items.contains_key("struct PonkoAfter") && items.contains_key("fn ponko_after"));
    // The closing brace of `extern "C"` is not mistaken for an item.
    assert_eq!(items.len(), 9, "{:?}", items.keys().collect::<Vec<_>>());
}

#[test]
fn identical_headers_are_compatible() {
    assert!(header_incompatibilities(MINI, MINI).is_empty());
    assert_eq!(parse_header(MINI).len(), 7);
}

#[test]
fn removing_or_changing_a_function_is_incompatible() {
    let without = MINI.replace("struct PonkoThing *ponko_thing_new(const char *name, size_t len);", "");
    expect_one(header_incompatibilities(MINI, &without), "fn ponko_thing_new: removed");
    let ret = changed(
        "struct PonkoThing *ponko_thing_new",
        "const struct PonkoThing *ponko_thing_new",
    );
    expect_one(header_incompatibilities(MINI, &ret), "fn ponko_thing_new: changed");
    let extra = changed(
        "const char *name, size_t len)",
        "const char *name, size_t len, int flags)",
    );
    expect_one(header_incompatibilities(MINI, &extra), "fn ponko_thing_new: changed");
    let wider = changed("size_t len)", "uint64_t len)");
    expect_one(header_incompatibilities(MINI, &wider), "fn ponko_thing_new: changed");
}

#[test]
fn changing_a_struct_is_incompatible() {
    let added = changed("  float y;\n", "  float y;\n  float z;\n");
    expect_one(header_incompatibilities(MINI, &added), "struct PonkoPoint: changed");
    let swapped = changed("  float x;\n  float y;\n", "  float y;\n  float x;\n");
    expect_one(header_incompatibilities(MINI, &swapped), "struct PonkoPoint: changed");
    let retyped = changed("float y;", "double y;");
    expect_one(header_incompatibilities(MINI, &retyped), "struct PonkoPoint: changed");
}

#[test]
fn changing_an_enum_constant_or_a_define_is_incompatible() {
    let removed = changed("  PONKO_B,\n", "");
    expect_one(header_incompatibilities(MINI, &removed), "constant PONKO_B removed");
    let shifted = changed("  PONKO_A = 0,\n", "  PONKO_NEW,\n  PONKO_A,\n");
    expect_one(
        header_incompatibilities(MINI, &shifted),
        "constant PONKO_A changed value",
    );
    let define = changed("#define PONKO_NONE 0", "#define PONKO_NONE 1");
    expect_one(header_incompatibilities(MINI, &define), "define PONKO_NONE: changed");
    let callback = changed("void *user);", "int *user);");
    expect_one(
        header_incompatibilities(MINI, &callback),
        "typedef PonkoCallback: changed",
    );
}

#[test]
fn listed_unstable_items_may_change_and_the_rest_may_not() {
    let unstable = BTreeSet::from(["ponko_thing_new".to_string(), "PonkoPoint".to_string()]);
    let changed = MINI
        .replace("struct PonkoThing *ponko_thing_new(const char *name, size_t len);", "")
        .replace("  float y;\n", "");
    assert!(stable_incompatibilities(MINI, &changed, &unstable).is_empty());
    let moved = changed.replace("struct PonkoPoint by", "int by");
    expect_one(
        stable_incompatibilities(MINI, &moved, &unstable),
        "fn ponko_thing_move: changed",
    );
    // The real list names the experimental C functions, which the header marks.
    let listed = unstable_c_items();
    assert!(listed.contains("ponko_ctx_hdr_status") && listed.contains("ponko_net_messages_rtc_host"));
    assert!(current_header().contains("/* Unstable: experimental (book/src/hdr.md) */"));
}

#[test]
fn additions_are_compatible() {
    let more = format!(
        "{MINI}\ntypedef struct PonkoBox {{ float w; }} PonkoBox;\n\
         enum PonkoStatus ponko_thing_resize(struct PonkoThing *thing, struct PonkoBox to);\n"
    )
    .replace("  PONKO_B,\n", "  PONKO_B,\n  PONKO_C,\n")
    .replace("#define PONKO_NONE 0", "#define PONKO_NONE 0\n#define PONKO_MORE 7");
    assert_eq!(header_incompatibilities(MINI, &more), Vec::<String>::new());
}

#[test]
fn renaming_a_parameter_is_compatible() {
    let renamed = changed("const char *name, size_t len)", "const char *label, size_t length)");
    assert_eq!(header_incompatibilities(MINI, &renamed), Vec::<String>::new());
}
