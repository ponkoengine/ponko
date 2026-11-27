//! Link flags for the shared library (`libponko_ffi.so`).
//!
//! With `video` on arm64 Linux and Android, the AV1 decoder's assembly
//! (dav1d's, in `re_rav1d`) reads its tables (`dav1d_mc_subpel_filters` and
//! others) with PC-relative addresses. In rav1d they are Rust `#[no_mangle]`
//! statics, which a shared library exports; an exported symbol may be
//! replaced by another library's at load time, so the linker refuses a
//! PC-relative reference to it ("may bind externally ... recompile with
//! -fPIC"). `-Bsymbolic` binds the library's references to its own
//! definitions, which is what the assembly assumes (dav1d hides its
//! tables), so the library links with the assembly kept. Nothing else
//! changes: the C API never relied on a symbol of it being replaced from
//! outside.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let video = std::env::var_os("CARGO_FEATURE_VIDEO").is_some();
    if video && arch == "aarch64" && (os == "linux" || os == "android") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic");
    }
}
