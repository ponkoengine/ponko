//! Background jobs and cell grids, public in [`crate::streaming`]: [`Job`]
//! runs work on another thread, and `CellStreamer` (with the `3d` feature)
//! says which cells of a grid to load or drop as the camera moves, nearest
//! first.

#[cfg(feature = "3d")]
use std::collections::HashSet;
use std::sync::mpsc;

#[cfg(feature = "3d")]
use glam::{Vec2, Vec3};

/// Work running on a worker thread, and its result once it's done.
///
/// Start it with [`Job::spawn`], then check [`Job::take`] each frame: the
/// game keeps running meanwhile. GPU work (making meshes, textures) belongs
/// on the game's thread, after the job; building the data for it (heights,
/// `MeshData`, decoded images) belongs in the job. In a browser the
/// work runs on a worker thread when the build has threads and the page
/// allows them (see the guide's "Threads on the web"); otherwise it runs
/// when it's spawned.
///
/// ```
/// # use ponko::prelude::*;
/// let mut job = ponko::streaming::Job::spawn(|| (1..=10).sum::<i32>());
/// // Each frame, until it's done:
/// let sum = loop {
///     if let Some(sum) = job.take() {
///         break sum;
///     }
/// };
/// assert_eq!(sum, 55);
/// ```
#[derive(Debug)]
pub struct Job<T> {
    result: mpsc::Receiver<T>,
    /// Kept here once received, until taken (boxed, so a job stays small
    /// while it runs).
    done: Option<Box<T>>,
    taken: bool,
}

impl<T: Send + 'static> Job<T> {
    /// Starts `work` on a worker thread.
    ///
    /// Jobs have their own pool, of half as many threads as cores (at least
    /// one), apart from the pool for the frame's parallel work (such as
    /// `par_for_each`): busy jobs never make a frame wait for a free thread.
    /// At most that many jobs run at once; the rest wait their turn.
    /// Parallel work inside a job stays in the job pool. Dropping a job
    /// doesn't stop its work.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let heights = ponko::streaming::Job::spawn(|| vec![0.0f32; 129 * 129]);
    /// assert_eq!(heights.wait().len(), 129 * 129);
    /// ```
    pub fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::start(work, true)
    }

    /// [`spawn`](Self::spawn), except that in a browser the work runs at
    /// once on the page's thread even when the game has threads: for the
    /// engine's own jobs that use what only the page's thread has, such as
    /// the browser storage's database.
    pub(crate) fn spawn_on_web_page(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::start(work, false)
    }

    /// Starts `work`; `web_workers`: on a worker in a browser, if the game
    /// has threads.
    fn start(work: impl FnOnce() -> T + Send + 'static, web_workers: bool) -> Self {
        let (send, result) = mpsc::sync_channel(1);
        let run = move || {
            // A panic drops `send` unsent: taking the result then panics on
            // the game's thread (rayon would abort the process instead).
            if let Ok(result) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                // The game may have dropped the job: then nobody wants it.
                let _ = send.send(result);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = web_workers;
            match job_pool() {
                Some(pool) => pool.spawn(run),
                None => rayon::spawn(run),
            }
        }
        // The web: on a worker of the job pool when the game has threads.
        #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
        {
            let run: Box<dyn FnOnce() + Send> = Box::new(run);
            let here = if web_workers {
                crate::web_threads::spawn_job(run).err()
            } else {
                Some(run)
            };
            if let Some(run) = here {
                run();
            }
        }
        #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
        {
            let _ = web_workers;
            run();
        }
        Self {
            result,
            done: None,
            taken: false,
        }
    }
}

/// The pool [`Job`]s run on, built on first use: half as many threads as
/// cores, at least one. `None` if its threads couldn't be started (then
/// jobs fall back to the global pool, as before).
#[cfg(not(target_arch = "wasm32"))]
fn job_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<Option<rayon::ThreadPool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
        rayon::ThreadPoolBuilder::new()
            .num_threads(job_threads(cores))
            .thread_name(|i| format!("ponko-job-{i}"))
            .build()
            .ok()
    })
    .as_ref()
}

/// The job pool's threads next to `threads` others (the cores natively,
/// the frame threads on the web): half, at least one.
#[cfg_attr(all(target_arch = "wasm32", not(target_feature = "atomics")), allow(dead_code))]
pub(crate) fn job_threads(threads: usize) -> usize {
    (threads / 2).max(1)
}

/// The most frame threads a web build starts when the game doesn't say
/// ([`Game::web_threads`](crate::Game::web_threads)): every worker holds
/// its own instance of the module and a stack.
pub(crate) const WEB_DEFAULT_MAX_THREADS: usize = 16;

/// The frame threads a web build with threads starts: what the game asked
/// for, else one fewer than the browser's logical `cores` (the page's
/// thread waits while they work), at most [`WEB_DEFAULT_MAX_THREADS`].
/// Fewer than 2 means none: the game stays on the page's thread.
#[cfg_attr(not(all(target_arch = "wasm32", target_feature = "atomics")), allow(dead_code))]
pub(crate) fn web_thread_count(requested: Option<usize>, cores: usize) -> usize {
    let threads = requested.unwrap_or_else(|| cores.saturating_sub(1).min(WEB_DEFAULT_MAX_THREADS));
    if threads < 2 { 0 } else { threads }
}

impl<T> Job<T> {
    /// Whether the work has finished (its result may already be taken).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut job = ponko::streaming::Job::spawn(|| 7);
    /// while !job.is_done() {}
    /// assert!(job.is_done());
    /// ```
    pub fn is_done(&mut self) -> bool {
        self.poll();
        self.taken || self.done.is_some()
    }

    /// The result, once the work is done; `None` before then, and after it
    /// was taken. Never waits.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut job = ponko::streaming::Job::spawn(|| "ready");
    /// while !job.is_done() {}
    /// assert_eq!(job.take(), Some("ready"));
    /// assert_eq!(job.take(), None);
    /// ```
    ///
    /// # Panics
    ///
    /// If the work panicked.
    pub fn take(&mut self) -> Option<T> {
        self.poll();
        let result = self.done.take();
        if result.is_some() {
            self.taken = true;
        }
        result.map(|r| *r)
    }

    /// Waits for the result, blocking this thread (for loading screens and
    /// tests; a game's frames shouldn't wait).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// assert_eq!(ponko::streaming::Job::spawn(|| 2 + 2).wait(), 4);
    /// ```
    ///
    /// # Panics
    ///
    /// If the work panicked, or its result was already taken.
    pub fn wait(mut self) -> T {
        if let Some(result) = self.done.take() {
            return *result;
        }
        assert!(
            !self.taken,
            "ponko: Job::wait: the result was already taken; consume each job result only once"
        );
        recv(&self.result).unwrap_or_else(|_| {
            panic!("ponko: a job's work panicked; fix the panic in the closure passed to Job::spawn")
        })
    }

    fn poll(&mut self) {
        if self.done.is_some() || self.taken {
            return;
        }
        match self.result.try_recv() {
            Ok(result) => self.done = Some(Box::new(result)),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                panic!("ponko: a job's work panicked; fix the panic in the closure passed to Job::spawn")
            }
        }
    }
}

/// Waits for a job's result. The browser's own thread may not block, so
/// with threads it spins there instead.
fn recv<T>(result: &mpsc::Receiver<T>) -> Result<T, mpsc::RecvError> {
    #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
    if crate::web_threads::sends_to_workers() {
        let mut got = None;
        crate::web_threads::spin_until(|| match result.try_recv() {
            Ok(r) => {
                got = Some(Ok(r));
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                got = Some(Err(mpsc::RecvError));
                true
            }
        });
        return got.unwrap_or(Err(mpsc::RecvError));
    }
    result.recv()
}

/// Which cells of a square grid on the ground (X and Z) to have loaded
/// around a point, such as the camera: those within `load_radius`, started
/// nearest first, and kept until they're beyond `keep_radius`. The gap
/// between the two keeps cells at the edge from loading and dropping over
/// and over as the camera moves back and forth.
///
/// The streamer only keeps the list: the game loads and drops each cell
/// (often with a [`Job`]). Cell `(x, z)` covers `x * cell_size` to
/// `(x + 1) * cell_size` along X, and the same along Z.
///
/// ```
/// # use ponko::prelude::*;
/// let mut cells = ponko::streaming::CellStreamer::new(256.0, 600.0, 800.0);
/// cells.update(Vec3::ZERO, 100);
/// assert!(cells.to_load().contains(&(0, 0)));
/// assert_eq!(cells.to_load()[0], (-1, -1), "nearest first: all four touch the camera");
/// // Far away, the old cells go and new ones come.
/// cells.update(vec3(5000.0, 0.0, 0.0), 100);
/// assert!(cells.to_drop().contains(&(0, 0)));
/// ```
#[cfg(feature = "3d")]
#[derive(Debug, Clone)]
pub struct CellStreamer {
    cell_size: f32,
    load_radius: f32,
    keep_radius: f32,
    loaded: HashSet<(i32, i32)>,
    to_load: Vec<(i32, i32)>,
    to_drop: Vec<(i32, i32)>,
    /// Cells in reach and not yet loaded, with their distances.
    wanted: Vec<(f32, (i32, i32))>,
}

#[cfg(feature = "3d")]
impl CellStreamer {
    /// A streamer of cells `cell_size` wide, loading those within
    /// `load_radius` of the point and dropping those beyond `keep_radius`
    /// (raised to `load_radius` when it's less).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let cells = ponko::streaming::CellStreamer::new(128.0, 500.0, 700.0);
    /// assert!(cells.loaded().next().is_none());
    /// ```
    ///
    /// # Panics
    ///
    /// If `cell_size` isn't above 0, or a radius is negative.
    #[must_use]
    pub fn new(cell_size: f32, load_radius: f32, keep_radius: f32) -> Self {
        assert!(
            cell_size > 0.0 && cell_size.is_finite(),
            "ponko: CellStreamer: cell_size must be above 0 (got {cell_size})"
        );
        assert!(
            load_radius >= 0.0 && keep_radius >= 0.0,
            "ponko: CellStreamer: the radii can't be negative"
        );
        Self {
            cell_size,
            load_radius,
            keep_radius: keep_radius.max(load_radius),
            loaded: HashSet::new(),
            to_load: Vec::new(),
            to_drop: Vec::new(),
            wanted: Vec::new(),
        }
    }

    /// Works out the cells to load and drop around `at` (its height is
    /// ignored): at most `max_new` new ones this time, nearest first, so
    /// a game can spread the work over frames. Read them with
    /// [`CellStreamer::to_load`] and [`CellStreamer::to_drop`]; both count
    /// as done (loaded, dropped) from here on.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cells = ponko::streaming::CellStreamer::new(10.0, 25.0, 40.0);
    /// cells.update(Vec3::ZERO, 2);
    /// assert_eq!(cells.to_load().len(), 2, "two this frame");
    /// cells.update(Vec3::ZERO, 100);
    /// assert!(cells.to_load().len() > 2, "the rest the next");
    /// ```
    pub fn update(&mut self, at: Vec3, max_new: usize) {
        let at = Vec2::new(at.x, at.z);
        let size = self.cell_size;
        // How far `at` is from the nearest point of a cell.
        let away = |(x, z): (i32, i32)| {
            let min = Vec2::new(x as f32, z as f32) * size;
            (at.clamp(min, min + size) - at).length()
        };
        self.to_drop.clear();
        let keep = self.keep_radius;
        let dropped = &mut self.to_drop;
        self.loaded.retain(|&cell| {
            let stays = away(cell) <= keep;
            if !stays {
                dropped.push(cell);
            }
            stays
        });
        self.to_drop.sort_unstable();
        self.wanted.clear();
        let reach = (self.load_radius / size).ceil() as i32 + 1;
        let (cx, cz) = ((at.x / size).floor() as i32, (at.y / size).floor() as i32);
        for z in cz - reach..=cz + reach {
            for x in cx - reach..=cx + reach {
                let d = away((x, z));
                if d <= self.load_radius && !self.loaded.contains(&(x, z)) {
                    self.wanted.push((d, (x, z)));
                }
            }
        }
        // Nearest first; equal distances in a fixed order.
        self.wanted
            .sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.to_load.clear();
        for &(_, cell) in self.wanted.iter().take(max_new) {
            self.loaded.insert(cell);
            self.to_load.push(cell);
        }
    }

    /// The cells the last [`CellStreamer::update`] started loading, nearest first.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cells = ponko::streaming::CellStreamer::new(1.0, 0.4, 0.4);
    /// cells.update(vec3(0.5, 0.0, 0.5), 10);
    /// assert_eq!(cells.to_load(), &[(0, 0)]);
    /// ```
    #[must_use]
    pub fn to_load(&self) -> &[(i32, i32)] {
        &self.to_load
    }

    /// The cells the last [`CellStreamer::update`] dropped, in grid order.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cells = ponko::streaming::CellStreamer::new(1.0, 0.4, 0.4);
    /// cells.update(vec3(0.5, 0.0, 0.5), 10);
    /// cells.update(vec3(10.5, 0.0, 0.5), 10);
    /// assert_eq!(cells.to_drop(), &[(0, 0)]);
    /// ```
    #[must_use]
    pub fn to_drop(&self) -> &[(i32, i32)] {
        &self.to_drop
    }

    /// Every cell loaded now (or being loaded), in no order.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cells = ponko::streaming::CellStreamer::new(1.0, 0.4, 0.4);
    /// cells.update(vec3(0.5, 0.0, 0.5), 10);
    /// assert_eq!(cells.loaded().count(), 1);
    /// ```
    pub fn loaded(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.loaded.iter().copied()
    }

    /// Whether `cell` is loaded (or being loaded).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cells = ponko::streaming::CellStreamer::new(1.0, 0.4, 0.4);
    /// cells.update(vec3(0.5, 0.0, 0.5), 10);
    /// assert!(cells.is_loaded((0, 0)) && !cells.is_loaded((5, 5)));
    /// ```
    #[must_use]
    pub fn is_loaded(&self, cell: (i32, i32)) -> bool {
        self.loaded.contains(&cell)
    }

    /// How many cells are loaded (or being loaded).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let cells = ponko::streaming::CellStreamer::new(1.0, 0.4, 0.4);
    /// assert_eq!(cells.loaded_count(), 0);
    /// ```
    #[must_use]
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Where cell `cell` starts: its corner with the lowest X and Z, at
    /// height 0.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let cells = ponko::streaming::CellStreamer::new(256.0, 0.0, 0.0);
    /// assert_eq!(cells.corner((2, -1)), vec3(512.0, 0.0, -256.0));
    /// ```
    #[must_use]
    pub fn corner(&self, (x, z): (i32, i32)) -> Vec3 {
        Vec3::new(x as f32 * self.cell_size, 0.0, z as f32 * self.cell_size)
    }

    /// Forgets every cell, as if none were loaded (after a teleport that
    /// dropped them all, or a new level).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cells = ponko::streaming::CellStreamer::new(1.0, 2.0, 2.0);
    /// cells.update(Vec3::ZERO, 100);
    /// cells.clear();
    /// assert_eq!(cells.loaded().count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.loaded.clear();
        self.to_load.clear();
        self.to_drop.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_threads_leave_one_core_to_the_page_and_two_cores_get_none() {
        assert_eq!(web_thread_count(None, 8), 7);
        assert_eq!(web_thread_count(None, 64), WEB_DEFAULT_MAX_THREADS);
        assert_eq!(web_thread_count(None, 2), 0);
        assert_eq!(web_thread_count(None, 0), 0);
        assert_eq!(web_thread_count(Some(1), 8), 0);
        assert_eq!(web_thread_count(Some(4), 2), 4);
    }

    #[test]
    fn the_job_pool_has_half_the_threads_and_at_least_one() {
        assert_eq!(job_threads(14), 7);
        assert_eq!(job_threads(7), 3);
        assert_eq!(job_threads(1), 1);
        assert_eq!(job_threads(0), 1);
    }
}
