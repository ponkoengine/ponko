use ponko::prelude::*;
use std::sync::{Arc, Mutex};

fn failure() -> Error {
    Error::new(ErrorKind::NotFound, "startup asset missing").with_path("missing.json")
}

#[test]
fn first_error_preserves_details_and_stops_callbacks_and_systems() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let first = log.clone();
    let never = log.clone();
    let systems = log.clone();
    let result = Game::new("setup", 1, 1)
        .setup(move |_| first.lock().unwrap().push(1))
        .try_setup(|_| Err(failure()))
        .setup(move |_| never.lock().unwrap().push(2))
        .sim_system(move |_| systems.lock().unwrap().push(3))
        .try_run_headless(5);
    assert_eq!(result.err(), Some(failure()));
    assert_eq!(*log.lock().unwrap(), vec![1]);
}

struct Legacy;
impl Plugin for Legacy {
    fn build(&self, game: Game) -> Game {
        game
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.insert_resource(42_u32);
    }
}
struct Failing;
impl Plugin for Failing {
    fn build(&self, game: Game) -> Game {
        game
    }
    fn try_setup(&self, ctx: &mut Ctx) -> Result<(), Error> {
        assert_eq!(*ctx.world.resource::<u32>(), 42);
        Err(failure())
    }
}

#[test]
fn plugin_default_setup_and_fallible_override_share_registration_order() {
    let result = Game::new("plugins", 1, 1)
        .plugin(Legacy)
        .plugin(Failing)
        .try_run_headless::<_, ()>(0, |_| panic!("must not run"));
    assert_eq!(result.err(), Some(failure()));
}

#[test]
fn successful_fallible_setup_runs_even_at_zero_ticks() {
    let world = Game::new("setup", 1, 1)
        .try_setup(|ctx| {
            let value = Ok::<_, Error>(7_u32)?;
            ctx.world.insert_resource(value);
            Ok(())
        })
        .try_run_headless(0)
        .unwrap();
    assert_eq!(*world.resource::<u32>(), 7);
}

#[test]
#[should_panic(expected = "ponko: startup asset missing")]
fn convenience_runner_panics_with_setup_error_message() {
    Game::new("setup", 1, 1).try_setup(|_| Err(failure())).run_headless(0);
}

#[test]
#[should_panic(expected = "user bug")]
fn fallible_runner_does_not_swallow_user_panics() {
    let _ = Game::new("setup", 1, 1)
        .try_setup(|_| panic!("user bug"))
        .try_run_headless(0);
}
