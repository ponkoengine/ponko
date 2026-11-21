//! The browser entry point of a C or C++ game: `web/build-cpp.sh` compiles
//! the game with the WASI SDK's clang and links its objects into this
//! program, built for `wasm32-unknown-unknown` (the guide's "C and C++ on the
//! web"). `main` calls the game's `main`.
//!
//! The C and C++ standard libraries come from the WASI SDK and call a few
//! WASI functions; the ones a game needs in a browser are defined below
//! (console output, clocks, random bytes, no files), so the module imports
//! nothing but the engine's own browser calls. `web/build-cpp.sh` stops with
//! the names of any other WASI function the game's code calls.
//!
//! On the desktop this program only says what it is for.

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {
    unsafe extern "C" {
        // The C program's `main`: clang names `int main(void)` so for
        // WebAssembly, and the C library's `__main_void` calls
        // `main(argc, argv)` when that is the one defined.
        fn __main_void() -> std::ffi::c_int;
        // Runs the C++ static constructors. Calling it here also keeps the
        // linker from wrapping every export in a call to it, which would
        // construct the game's globals again on every call from the page.
        fn __wasm_call_ctors();
    }
    // SAFETY: called once, before any of the game's code.
    unsafe { __wasm_call_ctors() };
    ponko_ffi::web::run_main(__main_void);
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn main() {
    eprintln!("web_main is the browser entry point of C and C++ games: build one with web/build-cpp.sh");
}

/// The WASI functions of the C library (`wasi_snapshot_preview1`), as the
/// C library declares them: the linker uses these definitions instead of
/// importing them. Each returns a WASI error number (0: success).
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasi {
    use std::cell::RefCell;

    use wasm_bindgen::prelude::*;

    const SUCCESS: i32 = 0;
    /// No such file descriptor: there are no files, and no folders to open.
    const EBADF: i32 = 8;
    const EINVAL: i32 = 28;
    const FILETYPE_CHARACTER_DEVICE: u8 = 2;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console, js_name = log)]
        fn console_log(line: &str);
        #[wasm_bindgen(js_namespace = console, js_name = error)]
        fn console_error(line: &str);
        #[wasm_bindgen(js_namespace = Date, js_name = now)]
        fn date_now() -> f64;
        #[wasm_bindgen(js_namespace = performance, js_name = now)]
        fn performance_now() -> f64;
        #[wasm_bindgen(js_namespace = crypto, js_name = getRandomValues)]
        fn get_random_values(bytes: &mut [u8]);
    }

    thread_local! {
        /// Unfinished lines of standard output and standard error.
        static LINES: RefCell<[Vec<u8>; 2]> = const { RefCell::new([Vec::new(), Vec::new()]) };
    }

    /// Standard output goes to the console's log, standard error to its
    /// errors, a line at a time.
    fn write_console(fd: i32, bytes: &[u8]) {
        LINES.with_borrow_mut(|lines| {
            let line = &mut lines[if fd == 1 { 0 } else { 1 }];
            for &byte in bytes {
                if byte == b'\n' {
                    let text = String::from_utf8_lossy(line);
                    if fd == 1 { console_log(&text) } else { console_error(&text) }
                    line.clear();
                } else {
                    line.push(byte);
                }
            }
        });
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_write(
        fd: i32,
        iovs: *const [u32; 2],
        iovs_len: usize,
        written: *mut usize,
    ) -> i32 {
        if fd != 1 && fd != 2 {
            return EBADF;
        }
        let mut total = 0;
        for i in 0..iovs_len {
            // SAFETY: the C library passes `iovs_len` (pointer, length) pairs of its own memory.
            let [ptr, len] = unsafe { iovs.add(i).read() };
            // SAFETY: as above: each pair is a readable buffer.
            let bytes = unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
            write_console(fd, bytes);
            total += bytes.len();
        }
        // SAFETY: the C library passes a writable count.
        unsafe { written.write(total) };
        SUCCESS
    }

    /// Standard input, output and error are terminals (so output is line
    /// buffered); there are no other descriptors.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_fdstat_get(fd: i32, stat: *mut u8) -> i32 {
        if !(0..=2).contains(&fd) {
            return EBADF;
        }
        // SAFETY: the C library passes a writable, 8-byte aligned 24-byte
        // `fdstat`: the file type at 0, flags at 2, then two rights masks.
        unsafe {
            stat.write_bytes(0, 24);
            stat.write(FILETYPE_CHARACTER_DEVICE);
        }
        SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_fd_close(_fd: i32) -> i32 {
        EBADF
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_fd_seek(_fd: i32, _offset: i64, _whence: i32, _to: *mut u64) -> i32 {
        EBADF
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_fd_read(_fd: i32, _iovs: *const u32, _len: usize, _read: *mut usize) -> i32 {
        EBADF
    }

    /// No folders are open: the C library finds no files.
    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_fd_prestat_get(_fd: i32, _prestat: *mut u8) -> i32 {
        EBADF
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_fd_prestat_dir_name(_fd: i32, _path: *mut u8, _len: usize) -> i32 {
        EBADF
    }

    /// No arguments and no environment variables.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_args_sizes_get(count: *mut usize, size: *mut usize) -> i32 {
        // SAFETY: the C library passes two writable counts.
        unsafe {
            count.write(0);
            size.write(0);
        }
        SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_args_get(_argv: *mut *mut u8, _buffer: *mut u8) -> i32 {
        SUCCESS
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_sizes_get(count: *mut usize, size: *mut usize) -> i32 {
        // SAFETY: as in args_sizes_get.
        unsafe { __imported_wasi_snapshot_preview1_args_sizes_get(count, size) }
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_environ_get(_environ: *mut *mut u8, _buffer: *mut u8) -> i32 {
        SUCCESS
    }

    /// The real-time clock (0) is `Date.now()`, the others `performance.now()`,
    /// in nanoseconds.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_clock_time_get(id: i32, _precision: i64, time: *mut u64) -> i32 {
        let ms = match id {
            0 => date_now(),
            1..=3 => performance_now(),
            _ => return EINVAL,
        };
        // SAFETY: the C library passes a writable time.
        unsafe { time.write((ms * 1e6) as u64) };
        SUCCESS
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_clock_res_get(id: i32, resolution: *mut u64) -> i32 {
        if !(0..=3).contains(&id) {
            return EINVAL;
        }
        // SAFETY: the C library passes a writable resolution.
        unsafe { resolution.write(1_000) };
        SUCCESS
    }

    /// `crypto.getRandomValues`, which gives at most 65536 bytes a call.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn __imported_wasi_snapshot_preview1_random_get(buffer: *mut u8, len: usize) -> i32 {
        // SAFETY: the C library passes a writable buffer of `len` bytes.
        let bytes = unsafe { std::slice::from_raw_parts_mut(buffer, len) };
        for chunk in bytes.chunks_mut(65_536) {
            get_random_values(chunk);
        }
        SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_sched_yield() -> i32 {
        SUCCESS
    }

    /// `exit`: the page can't end the program, so this stops it with an
    /// exception that names the code.
    #[unsafe(no_mangle)]
    extern "C" fn __imported_wasi_snapshot_preview1_proc_exit(code: i32) -> ! {
        wasm_bindgen::throw_str(&format!("the game called exit({code})"))
    }
}
