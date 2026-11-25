//! Sliced loaders from C (`PonkoSliceLoader`): an asset group decodes a
//! file of the game's own a piece at a time through C callbacks, hands the
//! finished state over once, fails with the loader's status, and frees
//! every state and the loader's `user` that nobody took.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use PonkoStatus::*;

/// What the callbacks count, the loader's `user`.
#[derive(Default)]
struct Counts {
    starts: AtomicUsize,
    steps: AtomicUsize,
    freed_states: AtomicUsize,
    freed_users: AtomicUsize,
    /// Fail the step with this number (0: never).
    fail_at: usize,
}

/// A sum of numbers, one a line, a thousand lines a step.
struct Sum {
    lines: Vec<i64>,
    at: usize,
    total: i64,
}

unsafe extern "C" fn start(user: *mut c_void, bytes: *const u8, len: usize, out_state: *mut *mut c_void) -> u32 {
    let counts = unsafe { &*user.cast::<Counts>() };
    counts.starts.fetch_add(1, SeqCst);
    let text = unsafe { std::slice::from_raw_parts(bytes, len) };
    let Ok(text) = std::str::from_utf8(text) else {
        return PONKO_INVALID_ARGUMENT as u32;
    };
    let lines = text.lines().map(|l| l.trim().parse().unwrap_or(0)).collect();
    let sum = Box::new(Sum { lines, at: 0, total: 0 });
    unsafe { out_state.write(Box::into_raw(sum).cast()) };
    PONKO_OK as u32
}

unsafe extern "C" fn step(user: *mut c_void, state: *mut c_void, out_done: *mut bool) -> u32 {
    let counts = unsafe { &*user.cast::<Counts>() };
    let n = counts.steps.fetch_add(1, SeqCst) + 1;
    if n == counts.fail_at {
        return PONKO_INVALID_ARGUMENT as u32;
    }
    let sum = unsafe { &mut *state.cast::<Sum>() };
    let end = (sum.at + 1000).min(sum.lines.len());
    sum.total += sum.lines[sum.at..end].iter().sum::<i64>();
    sum.at = end;
    unsafe { out_done.write(end == sum.lines.len()) };
    PONKO_OK as u32
}

unsafe extern "C" fn free_state(user: *mut c_void, state: *mut c_void) {
    let counts = unsafe { &*user.cast::<Counts>() };
    counts.freed_states.fetch_add(1, SeqCst);
    drop(unsafe { Box::from_raw(state.cast::<Sum>()) });
}

unsafe extern "C" fn free_user(user: *mut c_void) {
    let counts = unsafe { &*user.cast::<Counts>() };
    counts.freed_users.fetch_add(1, SeqCst);
}

fn loader(counts: &Counts) -> PonkoSliceLoader {
    PonkoSliceLoader {
        struct_size: size_of::<PonkoSliceLoader>() as u32,
        version: PONKO_SLICE_LOADER_VERSION,
        start: Some(start),
        step: Some(step),
        free_state: Some(free_state),
        user: std::ptr::from_ref(counts).cast_mut().cast(),
        free_user: Some(free_user),
    }
}

/// 2500 lines of numbers 1 to 2500.
fn numbers() -> Vec<u8> {
    (1..=2500).map(|n| format!("{n}\n")).collect::<String>().into_bytes()
}

unsafe fn group_with(ctx: *mut PonkoCtx, source: &CStr, counts: &Counts) -> u32 {
    unsafe {
        let files = ponko_memory_source_new(source.as_ptr());
        let bytes = numbers();
        assert_eq!(
            ponko_memory_source_insert(files, c"numbers.txt".as_ptr(), bytes.as_ptr(), bytes.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_background_memory_source(ctx, files), PONKO_OK);
        ponko_memory_source_free(files);
        let group = ponko_asset_group_new(c"data".as_ptr());
        let l = loader(counts);
        assert_eq!(
            ponko_asset_group_add_sliced(group, c"numbers.txt".as_ptr(), &l),
            PONKO_OK,
            "{}",
            last_error()
        );
        let mut id = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut id), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        id
    }
}

unsafe fn group_state(ctx: *mut PonkoCtx, id: u32) -> u32 {
    let mut state = 99;
    assert_eq!(unsafe { ponko_ctx_asset_group_state(ctx, id, &mut state) }, PONKO_OK);
    state
}

#[test]
fn one_shot_decoder_hands_over_the_value_without_running_a_slice() {
    let counts = Counts::default();
    run_ctx(|ctx| unsafe {
        let files = ponko_memory_source_new(c"one shot".as_ptr());
        let bytes = numbers();
        assert_eq!(
            ponko_memory_source_insert(files, c"numbers.txt".as_ptr(), bytes.as_ptr(), bytes.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_background_memory_source(ctx, files), PONKO_OK);
        ponko_memory_source_free(files);
        let group = ponko_asset_group_new(c"one shot".as_ptr());
        let desc = PonkoSliceLoader {
            step: None,
            ..loader(&counts)
        };
        assert_eq!(
            ponko_asset_group_add_decoder(group, c"numbers.txt".as_ptr(), &desc),
            PONKO_OK
        );
        let mut id = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut id), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        let mut value = null_mut();
        assert_eq!(
            ponko_ctx_take_asset_group_decoded(ctx, id, c"numbers.txt".as_ptr(), &mut value),
            PONKO_OK
        );
        let value = Box::from_raw(value.cast::<Sum>());
        assert_eq!(value.lines.len(), 2500);
        assert_eq!(value.lines.last(), Some(&2500));
        let mut absent = null_mut();
        assert_eq!(
            ponko_ctx_take_asset_group_decoded(ctx, id, c"numbers.txt".as_ptr(), &mut absent),
            PONKO_NOT_FOUND
        );
    });
    assert_eq!(counts.starts.load(SeqCst), 1);
    assert_eq!(counts.steps.load(SeqCst), 0);
    assert_eq!(counts.freed_states.load(SeqCst), 0);
    assert_eq!(counts.freed_users.load(SeqCst), 1);
}

#[test]
fn a_sliced_c_loader_works_in_pieces_and_hands_over_its_state_once() {
    let counts = Counts::default();
    run_ctx(|ctx| unsafe {
        let id = group_with(ctx, c"a", &counts);
        assert_eq!(
            group_state(ctx, id),
            PonkoAssetGroupState::PONKO_GROUP_READY as u32,
            "{}",
            last_error()
        );
        assert_eq!(counts.starts.load(SeqCst), 1);
        assert_eq!(counts.steps.load(SeqCst), 3, "2500 lines, a thousand a step");
        let mut state = null_mut();
        assert_eq!(
            ponko_ctx_take_asset_group_sliced(ctx, id, c"numbers.txt".as_ptr(), &mut state),
            PONKO_OK,
            "{}",
            last_error()
        );
        let sum = Box::from_raw(state.cast::<Sum>());
        assert_eq!(sum.total, 2500 * 2501 / 2);
        assert_eq!(
            ponko_ctx_take_asset_group_sliced(ctx, id, c"numbers.txt".as_ptr(), &mut state),
            PONKO_NOT_FOUND,
            "taken once"
        );
        assert_eq!(
            ponko_ctx_take_asset_group_sliced(ctx, id, c"other.txt".as_ptr(), &mut state),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_take_asset_group_sliced(ctx, id, c"numbers.txt".as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
    assert_eq!(counts.freed_states.load(SeqCst), 0, "the taken state is the game's");
    assert_eq!(
        counts.freed_users.load(SeqCst),
        1,
        "user freed once, when the group was dropped"
    );
}

#[test]
fn a_finished_state_nobody_takes_is_freed_with_the_game() {
    let counts = Counts::default();
    run_ctx(|ctx| unsafe {
        let id = group_with(ctx, c"b", &counts);
        assert_eq!(group_state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_READY as u32);
    });
    assert_eq!(counts.freed_states.load(SeqCst), 1);
    assert_eq!(counts.freed_users.load(SeqCst), 1);
}

#[test]
fn a_failing_step_fails_the_group_and_frees_its_state() {
    let counts = Counts {
        fail_at: 2,
        ..Counts::default()
    };
    run_ctx(|ctx| unsafe {
        let id = group_with(ctx, c"c", &counts);
        assert_eq!(group_state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_FAILED as u32);
        assert_ne!(ponko_ctx_asset_group_error(ctx, id), PONKO_OK);
        assert!(last_error().contains("step returned status 1"), "{}", last_error());
        assert_eq!(last_path(), "numbers.txt");
        assert_eq!(counts.freed_states.load(SeqCst), 1, "the half-made state is freed");
    });
    assert_eq!(counts.freed_users.load(SeqCst), 1);
}

#[test]
fn a_sliced_loader_needs_start_and_step() {
    let counts = Counts::default();
    unsafe {
        let group = ponko_asset_group_new(c"data".as_ptr());
        let no_step = PonkoSliceLoader {
            step: None,
            ..loader(&counts)
        };
        assert_eq!(
            ponko_asset_group_add_sliced(group, c"a.txt".as_ptr(), &no_step),
            PONKO_INVALID_ARGUMENT
        );
        let v0 = PonkoSliceLoader {
            version: 0,
            ..loader(&counts)
        };
        assert_eq!(
            ponko_asset_group_add_sliced(group, c"a.txt".as_ptr(), &v0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_asset_group_add_sliced(group, c"a.txt".as_ptr(), std::ptr::null()),
            PONKO_INVALID_ARGUMENT
        );
        // Added, then freed without starting: user freed once.
        let l = loader(&counts);
        assert_eq!(ponko_asset_group_add_sliced(group, c"a.txt".as_ptr(), &l), PONKO_OK);
        ponko_asset_group_free(group);
    }
    assert_eq!(counts.freed_users.load(SeqCst), 1);
    assert_eq!(counts.starts.load(SeqCst), 0);
}
