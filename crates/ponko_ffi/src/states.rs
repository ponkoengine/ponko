//! Game states from C: one state machine of `uint32_t` states per game
//! (Rust's `Game::state::<u32>`), its on-enter and on-exit work, systems
//! that run only in a state, and the requests a system makes.
//!
//! The state changes only between ticks: a change a tick asks for applies
//! at its end; see the guide's "Game states" chapter. Systems with a
//! condition callback (`ponko_game_add_system_if`) are Rust's
//! `.run_if(|ctx| ...)`.
use std::ffi::{c_char, c_void};

use ponko::prelude::*;
use ponko::state::{Condition, State};

use crate::ctx::{ctx_access, out, with_ctx};
use crate::game::{build, ok, phase, system, system_name};
use crate::{PonkoCtx, PonkoGame, PonkoStatus, PonkoSystemFn, boundary, invalid};

/// A run condition: whether the system runs this time. `ctx` is valid
/// until it returns; read what you need, change nothing.
/// ```
/// let none: ponko_ffi::PonkoConditionFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoConditionFn = Option<unsafe extern "C" fn(ctx: *mut PonkoCtx, user: *mut c_void) -> bool>;

fn no_state() -> crate::Failure {
    invalid("the game has no state machine: call ponko_game_state first")
}

/// Gives the game a state machine whose states are numbers (an `enum` of
/// the game's), starting in `initial`: the `State<u32>` resource and the
/// systems that apply changes between ticks. `initial`'s
/// on-enter work runs at the start of the first tick. A second call is
/// `PONKO_INVALID_ARGUMENT`.
/// # Safety
/// `game` must be null or a live game.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_state(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_state(game: *mut PonkoGame, initial: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            if g.has_state::<u32>() {
                (g, Err(invalid("the game already has a state machine")))
            } else {
                ok(g.state(initial))
            }
        })
    }
}

/// How `ponko_game_on_enter` and `ponko_game_on_exit` register.
#[derive(Clone, Copy)]
enum When {
    Enter,
    Exit,
}

unsafe fn transition(
    game: *mut PonkoGame,
    when: When,
    state: u32,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            let Some(f) = system_fn else {
                return (g, Err(invalid("system is null")));
            };
            if !g.has_state::<u32>() {
                return (g, Err(no_state()));
            }
            match when {
                When::Enter => ok(g.on_enter(state, system(f, user))),
                When::Exit => ok(g.on_exit(state, system(f, user))),
            }
        })
    }
}

/// Runs `system_fn` each time the game enters `state`, between ticks,
/// after the work registered before it (Rust's `Game::on_enter`).
/// Without `ponko_game_state` first: `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `user` stays the caller's; the engine passes it to `system_fn`
/// and never frees it. It must stay valid until the game is freed (or its run
/// returns).
///
/// # Safety
/// Live game; `system_fn` and `user` must stay valid while the game runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_on_enter(std::ptr::null_mut(), 0, None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_on_enter(
    game: *mut PonkoGame,
    state: u32,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { transition(game, When::Enter, state, system_fn, user) }
}

/// Runs `system_fn` each time the game leaves `state`, before it changes
/// (`ponko_ctx_state` is still `state` inside it); Rust's `Game::on_exit`.
/// Without `ponko_game_state` first: `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `user` stays the caller's; the engine passes it to `system_fn`
/// and never frees it. It must stay valid until the game is freed (or its run
/// returns).
///
/// # Safety
/// Live game; `system_fn` and `user` must stay valid while the game runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_on_exit(std::ptr::null_mut(), 0, None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_on_exit(
    game: *mut PonkoGame,
    state: u32,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { transition(game, When::Exit, state, system_fn, user) }
}

/// Builds a UI screen for `ponko_game_state_screen`: adds a detached root
/// (`ponko_ctx_ui_add_panel` with `PONKO_UI_NONE` as the parent) and its
/// nodes to the screen's UI, and returns the root's id. `ctx` is valid
/// until it returns.
/// ```
/// let none: ponko_ffi::PonkoUiBuildFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoUiBuildFn = Option<unsafe extern "C" fn(ctx: *mut PonkoCtx, user: *mut c_void) -> u64>;

/// Shows a UI screen while the game is in `state` (Rust's
/// `Game::state_screen`): each time the game enters `state`, `build` makes
/// the screen and returns its root, which is pushed in `mode` (a
/// `PonkoUiScreenMode`); each time it leaves, the screen is taken down and
/// its nodes removed. When the player closes the screen (Escape, a
/// gamepad's East button, or a button marked with
/// `ponko_ctx_ui_set_closes_screen`), the state is left too: popped, if it
/// was pushed over another one; a state that was set has nothing to go back
/// to, so its screen comes straight back up. A `build` that returns
/// `PONKO_UI_NONE` or a node that can't be a screen (one with a parent, or
/// one already up) shows an empty screen instead. Without
/// `ponko_game_state` first, a null `build` or an unknown `mode`:
/// `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `user` stays the caller's; the engine passes it to `build_fn`
/// each time the state is entered and never frees it. It must stay valid until
/// the game is freed (or its run returns).
///
/// # Safety
/// Live game; `build` and `user` must stay valid while the game runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_state_screen(std::ptr::null_mut(), 1, 1, None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_state_screen(
    game: *mut PonkoGame,
    state: u32,
    mode: u32,
    build_fn: PonkoUiBuildFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            let Some(f) = build_fn else {
                return (g, Err(invalid("build is null")));
            };
            let Some(mode) = crate::ui_retained::screen_mode(mode) else {
                return (g, Err(invalid("unknown PonkoUiScreenMode")));
            };
            if !g.has_state::<u32>() {
                return (g, Err(no_state()));
            }
            ok(g.state_screen(state, mode, screen_builder(f, user)))
        })
    }
}

/// A C build function as `Game::state_screen`'s: a root that can't be a
/// screen becomes an empty one.
fn screen_builder(
    f: unsafe extern "C" fn(*mut PonkoCtx, *mut c_void) -> u64,
    user: *mut c_void,
) -> impl FnMut(&mut Ctx) -> UiId + 'static {
    move |ctx: &mut Ctx| {
        // SAFETY: the caller of ponko_game_state_screen promised `f` and `user` stay valid while the game runs.
        let id = UiId::from_bits(with_ctx(ctx, |c| unsafe { f(c, user) }));
        let detached = ctx.ui.rect(id).is_some() && ctx.ui.parent(id).is_none();
        if detached && !ctx.ui.is_screen(id) && !ctx.ui.is_hud(id) {
            id
        } else {
            ctx.ui.root().id()
        }
    }
}

/// When a system added by `add_if` runs.
enum RunIf {
    InState(u32),
    InStack(u32),
    Callback(PonkoConditionFn, *mut c_void),
}

unsafe fn add_if(
    game: *mut PonkoGame,
    phase_raw: u32,
    name: *const c_char,
    every: u32,
    run_if: RunIf,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        let checked = (|| {
            let f = system_fn.ok_or_else(|| invalid("system is null"))?;
            if let RunIf::Callback(None, _) = run_if {
                return Err(invalid("condition is null"));
            }
            let phase = phase(phase_raw)?;
            if every == 0 {
                return Err(invalid("every must be at least 1"));
            }
            if every > 1 && phase > Phase::SimLast {
                return Err(invalid("only fixed-tick phases take an interval"));
            }
            Ok((f, phase, system_name(name, "C system")?))
        })();
        build(game, |g| match checked {
            Ok(_) if !matches!(run_if, RunIf::Callback(..)) && !g.has_state::<u32>() => (g, Err(no_state())),
            Ok((f, phase, name)) => {
                let when = match run_if {
                    RunIf::InState(s) => in_state(s),
                    RunIf::InStack(s) => in_stack(s),
                    RunIf::Callback(cond, cond_user) => {
                        let cond = cond.expect("checked above");
                        Condition::native(move |ctx: &mut Ctx| {
                            // SAFETY: the caller of ponko_game_add_system_if promised `cond` and
                            // `cond_user` stay valid while the game runs.
                            with_ctx(ctx, |c| cond(c, cond_user))
                        })
                    }
                };
                ok(g.add(phase, system(f, user).named(name).every(every).run_if(when)))
            }
            Err(e) => (g, Err(e)),
        })
    }
}

/// `ponko_game_add_system` for a system that runs only while the game's
/// state is `state` (Rust's `.run_if(in_state(state))`): not while another
/// state is pushed over it. Without `ponko_game_state` first:
/// `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `user` stays the caller's; the engine passes it to `system_fn`
/// and never frees it. It must stay valid until the game is freed (or its run
/// returns).
///
/// # Safety
/// Live game; `system_fn` and `user` must stay valid while the game runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_add_system_in_state(std::ptr::null_mut(), 3, std::ptr::null(), 1, 0, None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_add_system_in_state(
    game: *mut PonkoGame,
    phase: u32,
    name: *const c_char,
    every: u32,
    state: u32,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { add_if(game, phase, name, every, RunIf::InState(state), system_fn, user) }
}

/// `ponko_game_add_system_in_state`, also running while other states are
/// pushed over `state` (Rust's `.run_if(in_stack(state))`): drawing the
/// level under a pause menu.
///
/// Ownership: `user` stays the caller's; the engine passes it to `system_fn`
/// and never frees it. It must stay valid until the game is freed (or its run
/// returns).
///
/// # Safety
/// Live game; `system_fn` and `user` must stay valid while the game runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_add_system_in_stack(std::ptr::null_mut(), 3, std::ptr::null(), 1, 0, None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_add_system_in_stack(
    game: *mut PonkoGame,
    phase: u32,
    name: *const c_char,
    every: u32,
    state: u32,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { add_if(game, phase, name, every, RunIf::InStack(state), system_fn, user) }
}

/// `ponko_game_add_system` for a system that runs only when
/// `condition(ctx, condition_user)` returns true, checked right before it
/// would run (Rust's `.run_if(|ctx: &Ctx| ...)`); it needs no state
/// machine. A skipped system runs nothing and its change filters show
/// everything since it last ran. Like every C system it runs alone; the
/// condition should only read. A null `condition` is
/// `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `condition_user` and `user` stay the caller's; the engine passes
/// them to `condition` and `system_fn` and never frees them. They must stay
/// valid until the game is freed (or its run returns).
///
/// # Safety
/// Live game; `condition`, `condition_user`, `system_fn` and `user` must
/// stay valid while the game runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_add_system_if(std::ptr::null_mut(), 3, std::ptr::null(), 1, None, std::ptr::null_mut(), None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_game_add_system_if(
    game: *mut PonkoGame,
    phase: u32,
    name: *const c_char,
    every: u32,
    condition: PonkoConditionFn,
    condition_user: *mut c_void,
    system_fn: PonkoSystemFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        add_if(
            game,
            phase,
            name,
            every,
            RunIf::Callback(condition, condition_user),
            system_fn,
            user,
        )
    }
}

unsafe fn with_state<R>(ctx: *mut PonkoCtx, f: impl FnOnce(&mut State<u32>) -> R) -> Result<R, crate::Failure> {
    // SAFETY: forwarded caller contract.
    unsafe {
        ctx_access(ctx, |c| {
            let state = c.world.get_resource_mut::<State<u32>>().ok_or_else(no_state)?;
            Ok(f(state))
        })
    }
}

/// Writes the game's current state to `*out_state`.
/// # Safety
/// `ctx` must be null or live, outside query callbacks; `out_state` writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_state(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_state(ctx: *mut PonkoCtx, out_state: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let state = with_state(ctx, |s| *s.get())?;
        out(out_state, state)
    })
}

/// Writes how many simulation ticks have run with the current state on
/// top, this one included (Rust's `State::ticks`): every change resets it,
/// a pause pushed over a state doesn't advance it. For timed states (a
/// fade, a splash screen): times `ponko_ctx_fixed_dt` gives seconds.
/// # Safety
/// `ctx` must be null or live, outside query callbacks; `out_ticks` writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_state_ticks(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_state_ticks(ctx: *mut PonkoCtx, out_ticks: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let ticks = with_state(ctx, |s| s.ticks())?;
        out(out_ticks, ticks)
    })
}

/// Writes whether `state` is the current state or one it was pushed over.
/// # Safety
/// `ctx` must be null or live, outside query callbacks; `out_in_stack` writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_state_in_stack(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_state_in_stack(
    ctx: *mut PonkoCtx,
    state: u32,
    out_in_stack: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let found = with_state(ctx, |s| s.in_stack(&state))?;
        out(out_in_stack, found)
    })
}

/// Asks to leave every state on the stack and enter `state`, at the end of
/// this tick (Rust's `State::set`). Setting the current state leaves
/// and enters it again. The last request of a tick wins.
/// # Safety
/// `ctx` must be null or live, outside query callbacks.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_state(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_state(ctx: *mut PonkoCtx, state: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_state(ctx, |s| s.set(state)) })
}

/// Asks to enter `state` over the current one, which stays without being
/// left, at the end of this tick (Rust's `State::push`).
/// # Safety
/// `ctx` must be null or live, outside query callbacks.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_push_state(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_push_state(ctx: *mut PonkoCtx, state: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_state(ctx, |s| s.push(state)) })
}

/// Asks to leave the current state for the one it was pushed over, at the
/// end of this tick (Rust's `State::pop`); dropped then if nothing is
/// below.
/// # Safety
/// `ctx` must be null or live, outside query callbacks.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_pop_state(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_pop_state(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_state(ctx, State::pop) })
}
