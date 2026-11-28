//! Game states and run conditions: title, loading, playing, paused.
//!
//! **Maturity: proven**, from the guide chapter `states.md`. Game: Coin Dash and Ember Hop,
//! packaged and played by the list of checks (recorded on macOS) (title, play, pause and end
//! screens). Tests: `tests/states.rs`. Platforms: Linux, macOS and Windows, headless (the workspace
//! tests in the list of checks). Graphics: not involved.
//!
//! # Design
//!
//! - **One state type per state machine, defined by the game**: usually an
//!   `enum` (`Title`, `Playing`, `Paused`, ...). [`Game::state`] gives the
//!   game one of them and its first value. Any type that is [`States`]
//!   works (`Clone + PartialEq + Debug + Send + Sync`, and serde, so that
//!   saves and lockstep checksums carry it); a game can have several.
//! - **The state is a resource**, [`State<S>`]: the current state, the
//!   states it was pushed over, and the change asked for. It is reflected,
//!   so world saves ([`Storage::save_world`](crate::storage::Storage::save_world)),
//!   tools and the lockstep checksum see it like any reflected resource,
//!   and loading a save brings the state back with the world.
//! - **Changes are requests**: [`State::set`], [`State::push`] and
//!   [`State::pop`] (or [`Ctx::set_state`], [`Ctx::push_state`],
//!   [`Ctx::pop_state`]) only record the change. The last request before
//!   it applies wins.
//! - **The state only changes between simulation ticks**, at two defined
//!   points: a change asked for **during a tick** applies at its end, after
//!   [`Phase::SimLast`](crate::game::Phase::SimLast); one asked for
//!   **between ticks** (in setup, by a frame system, by on-enter work)
//!   applies at the start of the next tick, before
//!   [`Phase::Input`](crate::game::Phase::Input). Systems of the engine's do
//!   it, two per state type, the first and the last of the tick (listed in
//!   plans and timings as `ponko::state::apply_state<S>`). So every system
//!   of a tick sees the same state, the frame drawn after a tick shows the
//!   state that tick asked for, and a game in lockstep changes state on the
//!   same tick everywhere. While the simulation is paused
//!   (`ctx.time.pause()`) no tick runs and no change applies, so a pause
//!   menu is a state of the game, not a paused clock.
//! - **On-enter and on-exit work**: [`Game::on_enter`] and
//!   [`Game::on_exit`] register `&mut Ctx` functions that run when the
//!   change applies, in registration order, with their commands applied
//!   after each. The first value's on-enter runs at the start of the first
//!   tick.
//! - **While-in systems** are ordinary systems with a run condition:
//!   `.run_if(in_state(S::Playing))`, see below.
//! - **A stack for pause over play**: [`State::push`] puts a state over the
//!   current one without leaving it (no on-exit for the state below), and
//!   [`State::pop`] leaves the top state (its on-exit) and goes back to the
//!   one below (no on-enter: it never left). [`State::set`] leaves every
//!   state on the stack, top first, then enters the new one.
//! - **Every set is a change**: setting the state it is already in, the
//!   same value, leaves and enters it again (a restart). So a state value
//!   never carries a timer that is set again every tick; a **timed state**
//!   (a fade, a death animation, a splash screen) reads how long it has
//!   been on top, [`State::ticks`] or [`Ctx::seconds_in_state`], which
//!   every change resets and a pause pushed over it doesn't advance. Data
//!   that changes while a state lasts lives in a resource or on entities;
//!   data fixed for the whole state (where a fade leads) may be part of
//!   the value.
//! - **Loading a world** ([`Ctx::load_world`](crate::ctx::Ctx::load_world))
//!   replaces the state with the saved one, like every other resource: no
//!   on-exit or on-enter work runs for that, since the world it would act
//!   on is already the loaded one. A game that needs it asks for a state
//!   after loading.
//!
//! # Run conditions
//!
//! [`IntoSystem::run_if`](crate::game::IntoSystem::run_if) adds a
//! [`Condition`] to a system; it runs only when every condition holds,
//! checked right before it would run.
//!
//! - [`in_state`] (the state is this one) and [`in_stack`] (it is this one or
//!   pushed over it) read only the [`State<S>`] resource. They add a read of
//!   it to the system's declared access, so the parallel scheduler orders the
//!   system after any system that writes it, and a view system with one still
//!   shares batches: the state only changes between ticks, so the
//!   condition gives the same answer anywhere in the tick.
//! - A closure condition (`|ctx: &Ctx| ...`) may read anything, so a view
//!   system with one runs alone, as a stage of its own
//!   ([`Alone::Condition`](crate::schedule::Alone::Condition)).
//! - A system that a condition skips runs nothing, keeps its place in the
//!   world's change history (its `Changed` filters show everything since
//!   it last ran) and uses up its change tick, so the scheduler on and off
//!   still come out the same.
//! - Systems without a condition pay nothing for the feature but an empty
//!   check per system per run.
//!
//! ```
//! use ponko::prelude::*;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
//! enum Screen { Title, Playing, Paused }
//!
//! #[derive(Default)]
//! struct Score(u32);
//!
//! let world = Game::new("states", 1, 1)
//!     .state(Screen::Title)
//!     .on_enter(Screen::Playing, |ctx: &mut Ctx| ctx.world.insert_resource(Score::default()))
//!     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| ctx.world.resource_mut::<Score>().0 += 1).run_if(in_state(Screen::Playing)))
//!     .add(Phase::AfterPhysics, |ctx: &mut Ctx| match ctx.time.sim_tick() {
//!         0 => ctx.set_state(Screen::Playing),
//!         5 => ctx.push_state(Screen::Paused),
//!         _ => {}
//!     })
//!     .run_headless(10, |_| {});
//! // Playing from tick 1, paused from tick 6: five ticks of play.
//! assert_eq!(world.resource::<Score>().0, 5);
//! assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Paused);
//! ```

#![deny(missing_docs)]

use std::cell::RefCell;
use std::fmt::Debug;
use std::rc::Rc;

use ponko_ecs::World;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::access::SystemAccess;
use crate::ctx::Ctx;

#[allow(unused_imports)] // doc links
use crate::game::Game;

/// What a game state type needs: compare, copy, print, share between
/// threads, and save. Implemented for every such type; derive
/// `Clone, PartialEq, Debug, Serialize, Deserialize` on the game's enum.
///
/// ```
/// use serde::{Deserialize, Serialize};
/// #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
/// enum Screen { Title, Loading, Playing, Paused }
///
/// fn is_states<S: ponko::state::States>() {}
/// is_states::<Screen>();
/// is_states::<u32>(); // plain values work too (the C API uses them)
/// ```
pub trait States: Clone + PartialEq + Debug + Send + Sync + Serialize + DeserializeOwned + 'static {}

impl<T: Clone + PartialEq + Debug + Send + Sync + Serialize + DeserializeOwned + 'static> States for T {}

/// A change of state asked for, applied at the end of the tick (or before
/// the next one, when asked for between ticks): [`State::next`].
///
/// ```
/// use ponko::state::{Next, State};
/// let mut state = State::new(0u8);
/// assert_eq!(state.next(), None);
/// state.pop();
/// assert_eq!(state.next(), Some(&Next::Pop));
/// assert_eq!(state.apply_next(), None, "nothing below: the pop is dropped");
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "S: States")]
pub enum Next<S> {
    /// Leave every state on the stack and enter this one ([`State::set`]).
    Set(S),
    /// Enter this one over the current state ([`State::push`]).
    Push(S),
    /// Leave the current state, back to the one below ([`State::pop`]).
    Pop,
}

/// The state of a game's state machine of type `S`: a resource, added by
/// [`Game::state`]. See the [module documentation](self).
///
/// Read it with [`Ctx::state`] (or `world.resource::<State<S>>()` in a
/// view system that declares it); ask for changes with [`set`](Self::set),
/// [`push`](Self::push) and [`pop`](Self::pop), which apply between
/// simulation ticks.
///
/// ```
/// use ponko::state::{Next, State};
/// let mut state = State::new(1u8);
/// state.push(2);
/// assert_eq!(state.get(), &1, "a change applies between ticks");
/// assert_eq!(state.next(), Some(&Next::Push(2)));
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "S: States")]
pub struct State<S> {
    current: S,
    /// The states the current one was pushed over, bottom first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    below: Vec<S>,
    /// The change asked for since the state last changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next: Option<Next<S>>,
    /// The current state's on-enter work ran (false only before the first tick).
    #[serde(default)]
    entered: bool,
    /// Ticks run with the current state on top: [`State::ticks`].
    #[serde(default)]
    ticks: u64,
    /// `ticks` of the states in `below`, kept for when they are back on top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    below_ticks: Vec<u64>,
}

impl<S: States> State<S> {
    /// A state machine in `initial`, which it hasn't entered yet: its
    /// on-enter work runs before the next tick. [`Game::state`] makes
    /// one; a game inserts one itself only to replace it (a test that
    /// starts in another state, say).
    ///
    /// ```
    /// use ponko::state::State;
    /// let state = State::new(0u8);
    /// assert_eq!(state.get(), &0);
    /// assert_eq!(state.ticks(), 0, "it has not run a tick yet");
    /// assert!(state.below().is_empty() && state.next().is_none());
    /// ```
    #[must_use]
    pub fn new(initial: S) -> Self {
        State {
            current: initial,
            below: Vec::new(),
            next: None,
            entered: false,
            ticks: 0,
            below_ticks: Vec::new(),
        }
    }

    /// The current state.
    ///
    /// ```
    /// use ponko::state::State;
    /// let mut state = State::new(1u8);
    /// state.set(2);
    /// assert_eq!(state.get(), &1, "the request waits for the end of the tick");
    /// state.apply_next();
    /// assert_eq!(state.get(), &2);
    /// ```
    #[must_use]
    pub fn get(&self) -> &S {
        &self.current
    }

    /// Whether the current state is `state`.
    ///
    /// ```
    /// use ponko::state::State;
    /// let mut state = State::new(1u8);
    /// state.push(2);
    /// state.apply_next();
    /// assert!(state.is(&2));
    /// assert!(!state.is(&1), "a state pushed over is covered, not current");
    /// ```
    #[must_use]
    pub fn is(&self, state: &S) -> bool {
        self.current == *state
    }

    /// Whether `state` is the current state or one it was pushed over.
    ///
    /// ```
    /// use ponko::state::State;
    /// let (playing, paused) = (1u8, 2u8);
    /// let mut state = State::new(playing);
    /// state.push(paused);
    /// state.apply_next();
    /// assert!(state.is(&paused) && !state.is(&playing));
    /// assert!(state.in_stack(&paused) && state.in_stack(&playing));
    /// ```
    #[must_use]
    pub fn in_stack(&self, state: &S) -> bool {
        self.current == *state || self.below.contains(state)
    }

    /// How many simulation ticks have run with the current state on top,
    /// this one included (1 in the first tick after a change; 0 before the
    /// first tick). Every change resets it: a [`set`](Self::set), the same
    /// state included, and a [`push`](Self::push). A [`pop`](Self::pop)
    /// brings back the count of the state below, which didn't run on while
    /// covered, so a pause doesn't count. It is saved with the state.
    ///
    /// A timed state (a death animation, a fade, a splash screen) reads its
    /// time from here, instead of keeping a timer in the state value: each
    /// `set` is a change, which leaves and enters the state again.
    /// [`Ctx::seconds_in_state`] gives it in seconds.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::state::State;
    /// let (playing, paused) = (1u8, 2u8);
    /// let world = Game::new("ticks", 1, 1)
    ///     .state(playing)
    ///     .sim_system(move |ctx| match ctx.time.sim_tick() {
    ///         3 => ctx.push_state(paused),
    ///         5 => ctx.pop_state::<u8>(),
    ///         _ => {}
    ///     })
    ///     .run_headless(8, |_| {});
    /// // Ticks 0-3 playing, 4-5 paused, 6-7 playing again: 6 playing ticks.
    /// assert_eq!(world.resource::<State<u8>>().ticks(), 6);
    /// ```
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// The states the current one was pushed over, bottom first; empty
    /// unless [`push`](Self::push) was used.
    ///
    /// ```
    /// use ponko::state::State;
    /// let (play, pause) = (1u8, 2u8);
    /// let mut state = State::new(play);
    /// assert!(state.below().is_empty());
    /// state.push(pause);
    /// state.apply_next();
    /// assert_eq!(state.below(), &[play]);
    /// ```
    #[must_use]
    pub fn below(&self) -> &[S] {
        &self.below
    }

    /// The change asked for, which applies at the end of this tick (or
    /// before the next one); `None` if there is none.
    ///
    /// ```
    /// use ponko::state::{Next, State};
    /// let mut state = State::new(1u8);
    /// assert_eq!(state.next(), None);
    /// state.set(2);
    /// assert_eq!(state.next(), Some(&Next::Set(2)));
    /// state.apply_next();
    /// assert_eq!(state.next(), None, "applying it clears the request");
    /// ```
    #[must_use]
    pub fn next(&self) -> Option<&Next<S>> {
        self.next.as_ref()
    }

    /// Asks to leave every state on the stack (top first) and enter
    /// `state`. Setting the current state leaves and enters it again.
    /// Replaces any change asked for before in this tick.
    ///
    /// ```
    /// use ponko::state::{Next, State};
    /// let (title, options, playing) = (0u8, 1u8, 2u8);
    /// let mut state = State::new(title);
    /// state.push(options);
    /// state.set(playing);
    /// assert_eq!(state.next(), Some(&Next::Set(playing)), "the last request wins");
    /// ```
    pub fn set(&mut self, state: S) {
        self.next = Some(Next::Set(state));
    }

    /// Asks to enter `state` over the current one, which stays on the stack
    /// without being left: a pause menu over play. Replaces any change
    /// asked for before in this tick.
    ///
    /// ```
    /// use ponko::state::State;
    /// let (playing, paused) = (1u8, 2u8);
    /// let mut state = State::new(playing);
    /// state.push(paused);
    /// state.apply_next();
    /// assert_eq!((state.get(), state.below()), (&paused, &[playing][..]));
    /// ```
    pub fn push(&mut self, state: S) {
        self.next = Some(Next::Push(state));
    }

    /// Asks to leave the current state and go back to the one it was
    /// pushed over. With nothing below, the request is dropped when it
    /// would apply. Replaces any change asked for before in this tick.
    ///
    /// ```
    /// use ponko::state::State;
    /// let (playing, paused) = (1u8, 2u8);
    /// let mut state = State::new(playing);
    /// state.push(paused);
    /// state.apply_next();
    /// state.pop();
    /// state.apply_next();
    /// assert_eq!(state.get(), &playing);
    /// ```
    pub fn pop(&mut self) {
        self.next = Some(Next::Pop);
    }

    /// Applies the change asked for to this resource now, **without** the
    /// on-enter and on-exit work, and returns it (`None` if there was none,
    /// or a pop with nothing below). The game does this itself between
    /// ticks, with the work; this is for tests that run a game's
    /// systems by hand, without ticks, and must move on to the next state.
    ///
    /// ```
    /// use ponko::state::{Next, State};
    /// let (title, playing, paused) = (0u8, 1, 2);
    /// let mut state = State::new(title);
    /// state.set(playing);
    /// state.apply_next();
    /// state.push(paused);
    /// assert_eq!(state.apply_next(), Some(Next::Push(paused)));
    /// assert_eq!((state.get(), state.below()), (&paused, &[playing][..]));
    /// ```
    pub fn apply_next(&mut self) -> Option<Next<S>> {
        let next = self.next.take()?;
        match next.clone() {
            Next::Set(s) => {
                self.below.clear();
                self.below_ticks.clear();
                self.current = s;
                self.ticks = 0;
            }
            Next::Push(s) => self.push_now(s),
            Next::Pop => {
                self.current = self.below.pop()?;
                self.ticks = self.below_ticks.pop().unwrap_or(0);
            }
        }
        Some(next)
    }

    /// Puts `s` on top now, keeping the current state and its tick count below.
    fn push_now(&mut self, s: S) {
        let below = std::mem::replace(&mut self.current, s);
        self.below.push(below);
        self.below_ticks.push(std::mem::take(&mut self.ticks));
    }
}

type TransitionWork<S> = (S, Box<dyn FnMut(&mut Ctx)>);

/// What makes a state machine of the game's: kept by the game builder,
/// shared with the system that applies its changes.
pub(crate) struct Transitions<S> {
    pub(crate) enter: Vec<TransitionWork<S>>,
    pub(crate) exit: Vec<TransitionWork<S>>,
}

pub(crate) type Shared<S> = Rc<RefCell<Transitions<S>>>;

/// A new, empty set of transitions.
pub(crate) fn transitions<S>() -> Shared<S> {
    Rc::new(RefCell::new(Transitions {
        enter: Vec::new(),
        exit: Vec::new(),
    }))
}

/// Runs the work registered for `state` in `list`, applying commands after each.
fn run<S: PartialEq>(list: &mut [TransitionWork<S>], state: &S, ctx: &mut Ctx) {
    for (s, f) in list.iter_mut() {
        if s == state {
            f(ctx);
            ctx.apply_commands();
        }
    }
}

fn state_mut<S: States>(ctx: &mut Ctx) -> &mut State<S> {
    ctx.world.resource_mut::<State<S>>()
}

/// Where state machine `S` changes, before (`start`) and after every tick:
/// enters the first state before the first tick, then applies the change
/// asked for; at the start of a tick it also counts the tick
/// ([`State::ticks`]). A world without the resource (the game replaced the
/// world) has nothing to apply.
pub(crate) fn apply_state<S: States>(ctx: &mut Ctx, transitions: &Shared<S>, start: bool) {
    apply_change(ctx, transitions);
    if start && let Some(state) = ctx.world.get_resource_mut::<State<S>>() {
        state.ticks += 1;
    }
}

fn apply_change<S: States>(ctx: &mut Ctx, transitions: &Shared<S>) {
    let Some(state) = ctx.world.get_resource_mut::<State<S>>() else {
        return;
    };
    let first = !std::mem::replace(&mut state.entered, true);
    let next = state.next.take();
    let current = first.then(|| state.current.clone());
    let mut t = transitions.borrow_mut();
    let Transitions { enter, exit } = &mut *t;
    if let Some(current) = current {
        run(enter, &current, ctx);
    }
    match next {
        None => {}
        Some(Next::Set(new)) => {
            loop {
                let current = state_mut::<S>(ctx).current.clone();
                run(exit, &current, ctx);
                let state = state_mut::<S>(ctx);
                match state.below.pop() {
                    Some(below) => state.current = below,
                    None => break,
                }
            }
            let state = state_mut::<S>(ctx);
            state.current = new.clone();
            state.ticks = 0;
            state.below_ticks.clear();
            run(enter, &new, ctx);
        }
        Some(Next::Push(new)) => {
            state_mut::<S>(ctx).push_now(new.clone());
            run(enter, &new, ctx);
        }
        Some(Next::Pop) if !state_mut::<S>(ctx).below.is_empty() => {
            let current = state_mut::<S>(ctx).current.clone();
            run(exit, &current, ctx);
            let state = state_mut::<S>(ctx);
            if let Some(below) = state.below.pop() {
                state.current = below;
                state.ticks = state.below_ticks.pop().unwrap_or(0);
            }
        }
        Some(Next::Pop) => {}
    }
}

// ---- Screens tied to a state ------------------------------------------------

/// The work [`Game::state_screen`] registers: on enter, on exit, and a frame
/// system, sharing the root of the screen that is up.
pub(crate) struct StateScreen<S> {
    /// On enter: build the screen and push it.
    pub(crate) enter: Box<dyn FnMut(&mut Ctx)>,
    /// On exit: take the screen down and remove its nodes.
    pub(crate) exit: Box<dyn FnMut(&mut Ctx)>,
    /// Every frame: a screen the player closed leaves the state.
    pub(crate) frame: Box<dyn FnMut(&mut Ctx)>,
    _state: std::marker::PhantomData<S>,
}

pub(crate) fn state_screen<S: States>(
    state: S,
    mode: crate::ui::ScreenMode,
    mut build: impl FnMut(&mut Ctx) -> crate::ui::UiId + 'static,
) -> StateScreen<S> {
    let up: Rc<std::cell::Cell<Option<crate::ui::UiId>>> = Rc::default();
    let (on_exit, on_frame) = (up.clone(), up.clone());
    StateScreen {
        enter: Box::new(move |ctx: &mut Ctx| {
            if let Some(old) = up.take() {
                ctx.ui.remove(old);
            }
            let root = build(ctx);
            ctx.ui.push_screen_with_mode(root, mode);
            up.set(Some(root));
        }),
        exit: Box::new(move |ctx: &mut Ctx| {
            let Some(root) = on_exit.take() else { return };
            if ctx.ui.top_screen() == Some(root) {
                ctx.ui.pop_screen(); // gives the focus back to the screen below
            }
            ctx.ui.remove(root);
        }),
        frame: Box::new(move |ctx: &mut Ctx| {
            let Some(root) = on_frame.get() else { return };
            if !ctx.ui.closed_this_frame(root) {
                return;
            }
            let machine = ctx.world.get_resource::<State<S>>();
            if machine.is_some_and(|m| m.get() == &state && !m.below().is_empty()) {
                ctx.pop_state::<S>();
            } else {
                // Nothing to go back to: the screen lasts as long as its state.
                ctx.ui.push_screen_with_mode(root, mode);
            }
        }),
        _state: std::marker::PhantomData,
    }
}

// ---- Run conditions ---------------------------------------------------------

/// When a system runs: [`IntoSystem::run_if`](crate::game::IntoSystem::run_if)
/// adds one. Made by [`in_state`], [`in_stack`], or from a closure
/// `|ctx: &Ctx| -> bool`; see the [module documentation](self#run-conditions).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::state::Condition;
/// # #[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
/// # enum Screen { Title, Playing }
/// let not_on_title: Condition = in_state(Screen::Title).not();
/// let late = Condition::new(|ctx: &Ctx| ctx.time.sim_tick() > 100);
/// # let _ = (not_on_title, late);
/// ```
#[must_use = "a condition does nothing until a system takes it with `run_if`"]
pub struct Condition(Kind);

enum Kind {
    /// Reads only the resources `declare` adds: the same answer anywhere
    /// in a phase, so it may share batches.
    World {
        test: Box<dyn Fn(&World) -> bool + Send + Sync>,
        declare: Vec<fn(SystemAccess) -> SystemAccess>,
    },
    /// Reads anything: the system runs alone. Given `&mut Ctx` only so that
    /// a native (C) condition can be handed the context handle native
    /// callbacks take; Rust closures get `&Ctx`.
    Ctx(Box<dyn FnMut(&mut Ctx) -> bool>),
}

impl Condition {
    /// A condition from a closure that may read all of `ctx`. A view system
    /// with one runs alone; prefer [`in_state`] where it says the same.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::state::Condition;
    /// let world = Game::new("closure", 1, 1)
    ///     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| {
    ///         *ctx.world.resource_mut::<u32>() += 1;
    ///     }).run_if(Condition::new(|ctx| ctx.time.sim_tick() >= 2)))
    ///     .run_headless(4, |ctx| ctx.world.insert_resource(0u32));
    /// assert_eq!(*world.resource::<u32>(), 2);
    /// ```
    pub fn new(mut f: impl FnMut(&Ctx) -> bool + 'static) -> Self {
        Condition(Kind::Ctx(Box::new(move |ctx: &mut Ctx| f(ctx))))
    }

    /// [`new`](Self::new) for a condition that is handed the context
    /// mutably: for the C and C++ bindings, whose callbacks all take the
    /// same context handle. It should only read. Rust code uses `new`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::state::Condition;
    /// let late = Condition::native(|ctx: &mut Ctx| ctx.time.sim_tick() > 100);
    /// # let _ = late;
    /// ```
    #[doc(hidden)]
    pub fn native(f: impl FnMut(&mut Ctx) -> bool + 'static) -> Self {
        Condition(Kind::Ctx(Box::new(f)))
    }

    /// Holds when `self` doesn't. A state condition stays one: it
    /// declares the same read and keeps a view system in its batch.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let world = Game::new("not", 1, 1)
    ///     .state(0u8)
    ///     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| *ctx.world.resource_mut::<u32>() += 1).run_if(in_state(0u8).not()))
    ///     .sim_system(|ctx| ctx.set_state(1u8))
    ///     .run_headless(3, |ctx| ctx.world.insert_resource(0u32));
    /// assert_eq!(*world.resource::<u32>(), 2, "not in state 0 from the second tick");
    /// ```
    #[allow(clippy::should_implement_trait)] // Fluent condition builder, alongside `or` and `run_if`.
    pub fn not(self) -> Self {
        match self.0 {
            Kind::World { test, declare } => Condition(Kind::World {
                test: Box::new(move |w| !test(w)),
                declare,
            }),
            Kind::Ctx(mut f) => Condition(Kind::Ctx(Box::new(move |ctx| !f(ctx)))),
        }
    }

    /// Holds when `self` or `other` does (`other` is only checked when
    /// `self` doesn't hold). Two state conditions make a state condition;
    /// with a closure on either side it is a closure condition.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let (title, playing, results) = (0u8, 1u8, 2u8);
    /// let world = Game::new("or", 1, 1)
    ///     .state(title)
    ///     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| *ctx.world.resource_mut::<u32>() += 1)
    ///         .run_if(in_state(playing).or(in_state(results))))
    ///     .sim_system(move |ctx| match ctx.time.sim_tick() {
    ///         0 => ctx.set_state(playing),
    ///         1 => ctx.set_state(results),
    ///         _ => ctx.set_state(title),
    ///     })
    ///     .run_headless(4, |ctx| ctx.world.insert_resource(0u32));
    /// assert_eq!(*world.resource::<u32>(), 2);
    /// ```
    pub fn or(self, other: Condition) -> Self {
        match (self.0, other.0) {
            (
                Kind::World {
                    test: a,
                    declare: mut da,
                },
                Kind::World { test: b, declare: db },
            ) => {
                da.extend(db);
                Condition(Kind::World {
                    test: Box::new(move |w| a(w) || b(w)),
                    declare: da,
                })
            }
            (a, b) => {
                let (mut a, mut b) = (Condition(a).into_ctx(), Condition(b).into_ctx());
                Condition(Kind::Ctx(Box::new(move |ctx| a(ctx) || b(ctx))))
            }
        }
    }

    fn into_ctx(self) -> Box<dyn FnMut(&mut Ctx) -> bool> {
        match self.0 {
            Kind::World { test, .. } => Box::new(move |ctx| test(&ctx.world)),
            Kind::Ctx(f) => f,
        }
    }

    /// Whether it may read anything (a closure): the system then runs alone.
    pub(crate) fn reads_ctx(&self) -> bool {
        matches!(self.0, Kind::Ctx(_))
    }

    /// `access` with what this condition reads added.
    pub(crate) fn declare(&self, mut access: SystemAccess) -> SystemAccess {
        if let Kind::World { declare, .. } = &self.0 {
            for d in declare {
                access = d(access);
            }
        }
        access
    }

    /// Whether it holds now, for a system in a batch: only conditions that
    /// read the world get there (a closure makes its system run alone).
    pub(crate) fn holds_in_batch(&self, world: &World) -> bool {
        match &self.0 {
            Kind::World { test, .. } => test(world),
            Kind::Ctx(_) => unreachable!("a system with a closure condition runs alone"),
        }
    }

    /// Whether it holds now.
    pub(crate) fn holds(&mut self, ctx: &mut Ctx) -> bool {
        match &mut self.0 {
            Kind::World { test, .. } => test(&ctx.world),
            Kind::Ctx(f) => f(ctx),
        }
    }
}

/// Anything [`IntoSystem::run_if`](crate::game::IntoSystem::run_if) takes:
/// a [`Condition`], or a closure `|ctx: &Ctx| -> bool` (which names its
/// parameter type, as systems do). `Takes` only keeps the two apart.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::state::IntoCondition;
/// // A `Condition` and a closure both convert; `run_if` takes either.
/// let a = in_state(1u8).into_condition();
/// let b = (|ctx: &Ctx| ctx.time.sim_tick() > 5).into_condition();
/// # let _ = (a, b);
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` isn't a run condition",
    label = "not a condition",
    note = "a condition is `in_state(..)`, `in_stack(..)`, or a closure `|ctx: &Ctx| -> bool`"
)]
pub trait IntoCondition<Takes> {
    /// The condition.
    fn into_condition(self) -> Condition;
}

impl IntoCondition<()> for Condition {
    fn into_condition(self) -> Condition {
        self
    }
}

impl<F: FnMut(&Ctx) -> bool + 'static> IntoCondition<Ctx> for F {
    fn into_condition(self) -> Condition {
        Condition::new(self)
    }
}

fn read_state<S: States>(access: SystemAccess) -> SystemAccess {
    access.read_resource::<State<S>>()
}

/// Holds while the current state of state machine `S` is `state`: a
/// system with `.run_if(in_state(S::Playing))` runs only while playing (not
/// while a pause is pushed over it). False when the world has no `State<S>`.
///
/// ```
/// use ponko::prelude::*;
/// let world = Game::new("condition", 1, 1).state(false)
///     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| {
///         *ctx.world.resource_mut::<u32>() += 1;
///     }).run_if(in_state(true)))
///     .sim_system(|ctx| ctx.set_state(true))
///     .run_headless(3, |ctx| ctx.world.insert_resource(0u32));
/// assert_eq!(*world.resource::<u32>(), 2);
/// ```
pub fn in_state<S: States>(state: S) -> Condition {
    Condition(Kind::World {
        test: Box::new(move |w| w.get_resource::<State<S>>().is_some_and(|s| s.current == state)),
        declare: vec![read_state::<S>],
    })
}

/// Holds while `state` is the current state of state machine `S` or one it
/// was pushed over: for work that goes on under a pause menu, such as
/// drawing the level. False when the world has no `State<S>`.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::state::State;
/// let world = Game::new("stack", 1, 1)
///     .state(1u8)
///     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| {
///         ctx.world.resource_mut::<Vec<u64>>().push(ctx.time.sim_tick());
///     }).run_if(in_stack(1u8)))
///     .sim_system(|ctx| { if ctx.time.sim_tick() == 0 { ctx.push_state(2u8); } })
///     .run_headless(3, |ctx| ctx.world.insert_resource(Vec::<u64>::new()));
/// assert_eq!(world.resource::<Vec<u64>>(), &[0, 1, 2]);
/// assert_eq!(world.resource::<State<u8>>().get(), &2);
/// ```
pub fn in_stack<S: States>(state: S) -> Condition {
    Condition(Kind::World {
        test: Box::new(move |w| w.get_resource::<State<S>>().is_some_and(|s| s.in_stack(&state))),
        declare: vec![read_state::<S>],
    })
}

// ---- Ctx shortcuts -----------------------------------------------------------

impl Ctx {
    /// The current state of the game's state machine `S`
    /// ([`Game::state`]).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("read", 1, 1)
    ///     .state(7u8)
    ///     .sim_system(|ctx| assert_eq!(ctx.state::<u8>(), &7))
    ///     .run_headless(1, |_| {});
    /// ```
    ///
    /// # Panics
    /// If the game has no state of type `S`.
    #[must_use]
    #[track_caller]
    pub fn state<S: States>(&self) -> &S {
        self.world
            .get_resource::<State<S>>()
            .unwrap_or_else(|| no_state::<S>())
            .get()
    }

    /// How long the current state of state machine `S` has been on top, in
    /// simulated seconds: [`State::ticks`] times the fixed step. For timed
    /// states: a fade, a death animation, a splash screen.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// # #[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    /// # enum Screen { Splash, Title }
    /// let world = Game::new("splash", 1, 1)
    ///     .fixed_hz(10)
    ///     .state(Screen::Splash)
    ///     .add(Phase::AfterPhysics, (|ctx: &mut Ctx| {
    ///         if ctx.seconds_in_state::<Screen>() >= 0.5 {
    ///             ctx.set_state(Screen::Title);
    ///         }
    ///     }).run_if(in_state(Screen::Splash)))
    ///     .run_headless(10, |_| {});
    /// // Five ticks of splash (0.1 s each), then the title.
    /// assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Title);
    /// assert_eq!(world.resource::<ponko::state::State<Screen>>().ticks(), 5);
    /// ```
    ///
    /// # Panics
    /// If the game has no state of type `S`.
    #[must_use]
    #[track_caller]
    pub fn seconds_in_state<S: States>(&self) -> f32 {
        let ticks = self
            .world
            .get_resource::<State<S>>()
            .unwrap_or_else(|| no_state::<S>())
            .ticks();
        ticks as f32 * self.time.fixed_dt()
    }

    /// Asks state machine `S` to leave every state on its stack and enter
    /// `state`, between this tick and the next: [`State::set`].
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::state::State;
    /// let world = Game::new("set", 1, 1)
    ///     .state(1u8)
    ///     .sim_system(|ctx| {
    ///         assert_eq!(ctx.state::<u8>(), &1, "the change waits for the end of the tick");
    ///         ctx.set_state(2u8);
    ///     })
    ///     .run_headless(1, |_| {});
    /// assert_eq!(world.resource::<State<u8>>().get(), &2);
    /// ```
    ///
    /// # Panics
    /// If the game has no state of type `S`.
    #[track_caller]
    pub fn set_state<S: States>(&mut self, state: S) {
        self.state_machine::<S>().set(state);
    }

    /// Asks state machine `S` to enter `state` over the current one,
    /// between this tick and the next: [`State::push`].
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::state::State;
    /// let world = Game::new("push", 1, 1)
    ///     .state(1u8)
    ///     .sim_system(|ctx| ctx.push_state(2u8))
    ///     .run_headless(1, |_| {});
    /// let state = world.resource::<State<u8>>();
    /// assert_eq!((state.get(), state.below()), (&2, &[1][..]));
    /// ```
    ///
    /// # Panics
    /// If the game has no state of type `S`.
    #[track_caller]
    pub fn push_state<S: States>(&mut self, state: S) {
        self.state_machine::<S>().push(state);
    }

    /// Asks state machine `S` to leave the current state for the one below,
    /// between this tick and the next: [`State::pop`].
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::state::State;
    /// let (playing, paused) = (1u8, 2u8);
    /// let world = Game::new("pause", 1, 1)
    ///     .state(playing)
    ///     .sim_system(move |ctx| match ctx.time.sim_tick() {
    ///         0 => ctx.push_state(paused),
    ///         1 => ctx.pop_state::<u8>(),
    ///         _ => {}
    ///     })
    ///     .run_headless(3, |_| {});
    /// let state = world.resource::<State<u8>>();
    /// assert_eq!((state.get(), state.below()), (&playing, &[][..]));
    /// ```
    ///
    /// # Panics
    /// If the game has no state of type `S`.
    #[track_caller]
    pub fn pop_state<S: States>(&mut self) {
        self.state_machine::<S>().pop();
    }

    #[track_caller]
    fn state_machine<S: States>(&mut self) -> &mut State<S> {
        match self.world.get_resource_mut::<State<S>>() {
            Some(state) => state,
            None => no_state::<S>(),
        }
    }
}

#[track_caller]
fn no_state<S>() -> ! {
    panic!(
        "the game has no state of type `{}`: add it with `Game::state`",
        std::any::type_name::<S>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_replace_each_other_until_applied() {
        let mut s = State::new(0u32);
        s.set(1);
        s.push(2);
        assert_eq!(s.next(), Some(&Next::Push(2)));
        assert_eq!(s.get(), &0);
    }

    #[test]
    fn state_round_trips_through_json_with_its_stack_and_request() {
        let mut s = State::new(3u32);
        s.below.push(1);
        s.entered = true;
        s.pop();
        let json = serde_json::to_value(&s).unwrap();
        let back: State<u32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }
}
