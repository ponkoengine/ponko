//! Threads in the browser, for a web build compiled with the `atomics`
//! target feature (the guide's "Threads on the web" has the recipe).
//!
//! On a cross-origin isolated page the game starts Web Workers that share
//! its memory: a rayon pool for the frame's parallel work (parallel queries
//! and the parallel scheduler's batches), a smaller one for jobs, and a
//! coordinator. On any other page, or when a worker can't start, the game
//! runs on the page's thread alone, as a build without threads does.
//!
//! The page's thread may never block: the browser forbids
//! `memory.atomic.wait` there, which every lock and every rayon call that
//! waits would use. So it never calls into rayon and never waits for a lock
//! a worker holds. A parallel loop started on it (through
//! [`ponko_ecs::set_web_runner`]) or a job goes into a one-message mailbox;
//! the coordinator, a worker that may block, takes it and hands it to the
//! pools; the page's thread spins on an atomic flag until a loop is done.
//! Threads of the pools call rayon directly, as on the desktop.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::{Closure, JsValue, wasm_bindgen};

/// The worker script `web/build.sh --threads` and `ponko package --web
/// --threads` put next to the page; a canvas's `data-worker` attribute
/// names another.
const WORKER_SCRIPT: &str = "ponko-worker.js";

/// How long the workers may take to start before the game gives up on them
/// and stays on one thread.
const START_TIMEOUT_MS: i32 = 20_000;

/// What a worker runs, handed to it as a pointer in its first message.
enum Task {
    /// One thread of a rayon pool.
    Pool(rayon::ThreadBuilder),
    /// The coordinator: takes the page thread's messages.
    Coordinator,
}

/// A message from the page's thread to the coordinator.
enum Message {
    /// A parallel loop for the frame pool; `done` is set to 1 once it ran.
    Run {
        op: *mut (dyn FnMut() + Send + 'static),
        done: *const AtomicU32,
    },
    /// A job for the job pool.
    Job(Box<dyn FnOnce() + Send>),
}

// SAFETY: `Run`'s pointers come from `run_on_pool`, whose caller spins
// until `done` is set: the loop (which is `Send`) and the flag outlive the
// coordinator's use of them.
unsafe impl Send for Message {}

/// The one-message mailbox the page's thread posts to (its only writer)
/// and the coordinator reads (its only reader).
struct Mailbox {
    /// 1 while it holds a message.
    full: AtomicU32,
    message: UnsafeCell<Option<Message>>,
}

// SAFETY: `message` is written only while `full` is 0, by the page's thread,
// and read only while `full` is 1, by the coordinator; `full` (SeqCst)
// orders the two.
unsafe impl Sync for Mailbox {}

static MAILBOX: Mailbox = Mailbox {
    full: AtomicU32::new(0),
    message: UnsafeCell::new(None),
};

/// The pools, once their workers are being started.
struct Pools {
    frame: rayon::ThreadPool,
    jobs: rayon::ThreadPool,
}

static POOLS: OnceLock<Pools> = OnceLock::new();

/// The coordinator's thread, which parks while the mailbox is empty.
/// Unparking never blocks, so the page's thread may do it.
static COORDINATOR: OnceLock<std::thread::Thread> = OnceLock::new();

/// The frame pool's threads, once every worker is up; 0 before (and for
/// good on a page without threads).
static FRAME_THREADS: AtomicUsize = AtomicUsize::new(0);

/// A worker stopped with an error (a panic aborts the worker); the game
/// stops too, as it does for a panic on one thread.
static FAILED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// This thread is one of the game's workers, not the page's own.
    static WORKER: Cell<bool> = const { Cell::new(false) };
    /// The page's `Worker` objects and their message handlers, kept alive.
    static HANDLES: RefCell<Vec<WorkerHandle>> = const { RefCell::new(Vec::new()) };
}

struct WorkerHandle {
    worker: web_sys::Worker,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
}

/// The frame pool's threads (0 without threads): how many threads
/// parallel queries and the scheduler's batches use.
pub(crate) fn frame_threads() -> usize {
    FRAME_THREADS.load(Ordering::SeqCst)
}

/// Whether this thread is one of the game's workers.
pub(crate) fn on_worker() -> bool {
    WORKER.get()
}

/// Whether work started on this thread is sent to the workers: threads
/// are up and this is the page's thread.
pub(crate) fn sends_to_workers() -> bool {
    frame_threads() > 0 && !on_worker()
}

/// Hands a job to the job pool: through the coordinator from the page's
/// thread, directly from a worker. `false` if there are no threads (the
/// caller runs it here).
pub(crate) fn spawn_job(job: Box<dyn FnOnce() + Send>) -> Result<(), Box<dyn FnOnce() + Send>> {
    if frame_threads() == 0 {
        return Err(job);
    }
    if on_worker() {
        match POOLS.get() {
            Some(pools) => pools.jobs.spawn(job),
            None => return Err(job),
        }
    } else {
        post(Message::Job(job));
    }
    Ok(())
}

/// Spins until `ready()` (the page's thread may not block), stopping the
/// game if a worker failed meanwhile.
pub(crate) fn spin_until(mut ready: impl FnMut() -> bool) {
    while !ready() {
        stop_if_failed();
        std::hint::spin_loop();
    }
}

/// Stops the game if a worker failed: its error is already in the console.
fn stop_if_failed() {
    if FAILED.load(Ordering::SeqCst) {
        stop("a worker thread of the game stopped with an error (see the error above)");
    }
}

/// Logs `why` and aborts. Not a panic: unwinding out of a wait could free
/// memory a worker still uses.
fn stop(why: &str) -> ! {
    web_sys::console::error_1(&JsValue::from_str(&format!("ponko: {why}; the game stops")));
    std::process::abort()
}

/// Posts `message` for the coordinator. Only the page's thread posts, so the
/// mailbox holds at most the previous message, which the coordinator takes
/// at once (it waits for nothing else).
fn post(message: Message) {
    spin_until(|| MAILBOX.full.load(Ordering::SeqCst) == 0);
    // SAFETY: `full` is 0, so the coordinator doesn't touch the message,
    // and this thread is the only writer.
    unsafe { *MAILBOX.message.get() = Some(message) };
    MAILBOX.full.store(1, Ordering::SeqCst);
    // Set before the first message (see `start_workers`).
    if let Some(coordinator) = COORDINATOR.get() {
        coordinator.unpark();
    }
}

/// [`ponko_ecs::WebRunner::run`]: runs `op` on the frame pool through the
/// coordinator, spinning until it has run.
fn run_on_pool(op: &mut (dyn FnMut() + Send)) {
    let done = AtomicU32::new(0);
    // SAFETY: only the lifetime changes; this function doesn't return
    // before the coordinator has set `done`, after its last use of `op`.
    let op: *mut (dyn FnMut() + Send + 'static) =
        unsafe { std::mem::transmute::<*mut (dyn FnMut() + Send + '_), *mut (dyn FnMut() + Send + 'static)>(op) };
    post(Message::Run { op, done: &done });
    spin_until(|| done.load(Ordering::SeqCst) != 0);
}

/// The coordinator's loop: waits for the page thread's messages and hands
/// them to the pools. It never calls rayon's global functions: the global
/// pool is the page's thread alone (see [`start`]).
fn coordinate(pools: &Pools) -> ! {
    /// A loop that runs on the pool: the page's thread waits for it.
    struct SendOp(*mut (dyn FnMut() + Send + 'static));
    // SAFETY: the loop itself is `Send`; see `Message`.
    unsafe impl Send for SendOp {}
    impl SendOp {
        /// A method, so the closure captures the whole (`Send`) wrapper.
        fn call(self) {
            // SAFETY: the page's thread keeps the loop alive until `done`.
            unsafe { (*self.0)() }
        }
    }
    let _ = COORDINATOR.set(std::thread::current());
    loop {
        while MAILBOX.full.load(Ordering::SeqCst) == 0 {
            // A worker may wait. An unpark that came first makes this
            // return at once.
            std::thread::park();
        }
        // SAFETY: `full` is 1: the page's thread wrote the message and
        // won't touch it until `full` is 0 again.
        let message = unsafe { (*MAILBOX.message.get()).take() };
        MAILBOX.full.store(0, Ordering::SeqCst);
        match message {
            Some(Message::Run { op, done }) => {
                let op = SendOp(op);
                pools.frame.install(move || op.call());
                // SAFETY: the page's thread spins until this store.
                unsafe { (*done).store(1, Ordering::SeqCst) };
            }
            Some(Message::Job(job)) => pools.jobs.spawn(job),
            None => {}
        }
    }
}

/// `game.js`'s `ponkoWorkerStart`, called by `ponko-worker.js` in each
/// worker once the module shares the page's memory: runs the worker's task,
/// which only returns if the game ends.
#[wasm_bindgen(js_name = ponkoWorkerStart)]
#[doc(hidden)]
pub fn worker_start(task: u32) {
    WORKER.set(true);
    // SAFETY: `task` is the `Box<Task>` `spawn_worker` leaked for this
    // worker and sent to it alone.
    let task = unsafe { Box::from_raw(task as usize as *mut Task) };
    match *task {
        Task::Pool(thread) => thread.run(),
        Task::Coordinator => match POOLS.get() {
            Some(pools) => coordinate(pools),
            None => stop("the coordinator started without thread pools"),
        },
    }
}

/// `game.js`'s `ponkoWorkerFailed`, called by `ponko-worker.js` when its
/// task throws (a panic in a worker aborts it): the page's thread stops the
/// game instead of waiting for that worker forever.
#[wasm_bindgen(js_name = ponkoWorkerFailed)]
#[doc(hidden)]
pub fn worker_failed() {
    FAILED.store(true, Ordering::SeqCst);
}

/// Starts the workers when the page allows it, before setup: returns once
/// they all run (then parallel work and jobs use them), or once it is clear
/// they won't (then the game stays on the page's thread, and the console
/// says why). `requested`: [`Game::web_threads`](crate::Game::web_threads).
pub(crate) async fn start(requested: Option<usize>) {
    match start_workers(requested).await {
        Ok(0) => {}
        Ok(threads) => web_sys::console::info_1(&JsValue::from_str(&format!(
            "ponko: running on {threads} worker threads (and {} for jobs)",
            crate::jobs::job_threads(threads)
        ))),
        Err(why) => web_sys::console::warn_1(&JsValue::from_str(&format!(
            "ponko: this build can use threads, but {why}; it runs on the page's thread alone"
        ))),
    }
}

async fn start_workers(requested: Option<usize>) -> Result<usize, String> {
    let window = web_sys::window().ok_or("there is no window")?;
    let cores = window.navigator().hardware_concurrency() as usize;
    let threads = crate::jobs::web_thread_count(requested, cores);
    if threads == 0 {
        // Asked for (`web_threads(1)`), or a machine with two cores: not
        // worth a warning.
        return Ok(0);
    }
    if !js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crossOriginIsolated")).is_ok_and(|v| v.is_truthy())
    {
        return Err("the page isn't cross-origin isolated (serve it with the headers \
                    `Cross-Origin-Opener-Policy: same-origin` and \
                    `Cross-Origin-Embedder-Policy: require-corp`)"
            .into());
    }
    let script = worker_script(&window)?;
    // The global rayon pool becomes the page's thread alone (rayon falls
    // back to that where threads can't be spawned): a rayon call made on
    // it outside a parallel loop then runs here, in order, instead of
    // waiting. Asked first, so no worker can make it its own.
    let _ = rayon::current_num_threads();

    let readies = js_sys::Array::new();
    let spawn = |name: String, task: Task| -> Result<(), String> {
        let (handle, ready) = spawn_worker(&script, &name, task).map_err(|e| describe(&e))?;
        HANDLES.with_borrow_mut(|h| h.push(handle));
        readies.push(&ready);
        Ok(())
    };
    let pools = (|| {
        let pool = |count: usize, prefix: &'static str| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(count)
                .thread_name(move |i| format!("{prefix}-{i}"))
                .spawn_handler(|thread| {
                    let name = thread.name().unwrap_or(prefix).to_string();
                    spawn(name, Task::Pool(thread)).map_err(std::io::Error::other)
                })
                .build()
                .map_err(|e| e.to_string())
        };
        Ok::<_, String>(Pools {
            frame: pool(threads, "ponko")?,
            jobs: pool(crate::jobs::job_threads(threads), "ponko-job")?,
        })
    })();
    let pools = match pools {
        Ok(pools) => pools,
        Err(e) => return Err(give_up(format!("a worker couldn't be started: {e}"))),
    };
    if POOLS.set(pools).is_err() {
        return Err(give_up("the workers were already started once".into()));
    }
    if let Err(e) = spawn("ponko-coordinator".into(), Task::Coordinator) {
        return Err(give_up(format!("a worker couldn't be started: {e}")));
    }
    let all = js_sys::Promise::all(&readies);
    let started = js_sys::Promise::race(&js_sys::Array::of2(&all, &timeout(&window, START_TIMEOUT_MS)));
    match wasm_bindgen_futures::JsFuture::from(started).await {
        Ok(v) if v.as_string().as_deref() == Some("timeout") => Err(give_up(format!(
            "the workers didn't start within {} s (is {script} served?)",
            START_TIMEOUT_MS / 1000
        ))),
        Ok(_) => {
            // The coordinator is running: wait (a moment) for it to say
            // which thread it is, so every message can wake it.
            spin_until(|| COORDINATOR.get().is_some());
            // SAFETY: `run_on_pool` runs its loop once and returns after
            // it; `on_worker` is true only on the workers, whose rayon calls
            // run on their own pool.
            unsafe {
                ponko_ecs::set_web_runner(ponko_ecs::WebRunner {
                    threads,
                    in_pool: on_worker,
                    run: run_on_pool,
                });
            }
            FRAME_THREADS.store(threads, Ordering::SeqCst);
            Ok(threads)
        }
        Err(e) => Err(give_up(format!("a worker failed to start: {}", describe(&e)))),
    }
}

/// Stops the workers started so far; returns `why`. Their pools stay
/// unused (nothing is sent to them).
fn give_up(why: String) -> String {
    HANDLES.with_borrow_mut(|handles| {
        for h in handles.drain(..) {
            h.worker.terminate();
        }
    });
    why
}

/// The worker script's URL: the canvas's `data-worker`, else
/// [`WORKER_SCRIPT`], relative to the page.
fn worker_script(window: &web_sys::Window) -> Result<String, String> {
    let document = window.document().ok_or("there is no document")?;
    let named = document
        .get_element_by_id("ponko")
        .and_then(|canvas| canvas.get_attribute("data-worker"))
        .filter(|s| !s.is_empty());
    let base = document.base_uri().ok().flatten().unwrap_or_default();
    let url =
        web_sys::Url::new_with_base(named.as_deref().unwrap_or(WORKER_SCRIPT), &base).map_err(|e| describe(&e))?;
    Ok(url.href())
}

/// Starts one worker on `task`; the promise resolves when the worker shares
/// the module (it says `ready`) and rejects if its script fails.
fn spawn_worker(script: &str, name: &str, task: Task) -> Result<(WorkerHandle, js_sys::Promise), JsValue> {
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    options.set_name(name);
    let worker = web_sys::Worker::new_with_options(script, &options)?;
    let resolve_slot = std::rc::Rc::new(RefCell::new(None::<(js_sys::Function, js_sys::Function)>));
    let ready = {
        let slot = resolve_slot.clone();
        js_sys::Promise::new(&mut move |resolve, reject| *slot.borrow_mut() = Some((resolve, reject)))
    };
    let on_message = {
        let slot = resolve_slot.clone();
        let name = name.to_string();
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data();
            if data.as_string().as_deref() == Some("ready") {
                if let Some((resolve, _)) = slot.borrow_mut().take() {
                    let _ = resolve.call0(&JsValue::NULL);
                }
            } else if let Ok(error) = js_sys::Reflect::get(&data, &JsValue::from_str("failed"))
                && !error.is_undefined()
            {
                // Between frames: the page's thread isn't waiting for the
                // worker, so it stops here.
                FAILED.store(true, Ordering::SeqCst);
                stop(&format!("the worker thread {name} stopped: {}", describe(&error)));
            }
        })
    };
    let on_error = {
        let slot = resolve_slot;
        let name = name.to_string();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let message = event
                .dyn_ref::<web_sys::ErrorEvent>()
                .map_or_else(|| "its script could not be loaded".to_string(), |e| e.message());
            if let Some((_, reject)) = slot.borrow_mut().take() {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&format!("{name}: {message}")));
            }
        })
    };
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    let task = Box::into_raw(Box::new(task));
    let message = js_sys::Object::new();
    let posted = (|| {
        js_sys::Reflect::set(&message, &JsValue::from_str("module"), &wasm_bindgen::module())?;
        js_sys::Reflect::set(&message, &JsValue::from_str("memory"), &wasm_bindgen::memory())?;
        js_sys::Reflect::set(
            &message,
            &JsValue::from_str("task"),
            &JsValue::from(task as usize as u32),
        )?;
        worker.post_message(&message)
    })();
    if let Err(e) = posted {
        // SAFETY: the message never reached the worker: the task is still ours.
        drop(unsafe { Box::from_raw(task) });
        worker.terminate();
        return Err(e);
    }
    Ok((
        WorkerHandle {
            worker,
            _on_message: on_message,
            _on_error: on_error,
        },
        ready,
    ))
}

/// A promise that resolves to `"timeout"` after `ms` milliseconds.
fn timeout(window: &web_sys::Window, ms: i32) -> js_sys::Promise {
    let window = window.clone();
    js_sys::Promise::new(&mut move |resolve, _| {
        let fire = Closure::once_into_js(move || {
            let _ = resolve.call1(&JsValue::NULL, &JsValue::from_str("timeout"));
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(fire.unchecked_ref(), ms);
    })
}

/// A JavaScript error or value as text.
fn describe(value: &JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}
