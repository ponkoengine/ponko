use ponko::prelude::*;

#[test]
fn windowed_fallible_runners_have_result_signatures_without_opening_windows() {
    fn direct() -> Result<(), Error> {
        Game::new("window", 1, 1).try_run(|_| {})
    }
    fn configured() -> Result<(), Error> {
        Game::new("window", 1, 1).configured().try_run()
    }
    let _: fn() -> Result<(), Error> = direct;
    let _: fn() -> Result<(), Error> = configured;
}

#[cfg(not(feature = "window"))]
#[test]
fn missing_window_feature_is_recoverable() {
    let error = Game::new("window", 1, 1).try_run(|_| {}).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.message().contains("window` feature is off"));
}

use ponko::error;
#[path = "../src/render/startup.rs"]
mod startup;

#[test]
fn gpu_startup_errors_return_directly_without_panicking() {
    // The hook is process-global: isolate it from other tests and verify that
    // every refused startup operation returns an error without calling it.
    const CHILD: &str = "PONKO_GPU_STARTUP_ERROR_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "gpu_startup_errors_return_directly_without_panicking"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "unexpected diagnostic: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let hooks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = hooks.clone();
    std::panic::set_hook(Box::new(move |_| {
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }));
    for (context, detail) in [
        ("could not create a GPU surface", "denied"),
        ("no compatible GPU found", "none"),
        ("could not open the GPU device", "denied"),
    ] {
        let error = startup::result(Err::<(), _>(detail), context).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert_eq!(error.to_string(), format!("ponko: {context}: {detail}"));
    }
    let error = startup::config(None::<()>).unwrap_err();
    assert_eq!(error.to_string(), "ponko: the GPU can't draw to this window");
    assert_eq!(startup::result(Ok::<_, &str>(42), "device"), Ok(42));
    assert_eq!(startup::config(Some(42)), Ok(42));
    assert_eq!(hooks.load(std::sync::atomic::Ordering::Relaxed), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn smoke_runner_returns_setup_errors_without_a_window() {
    const CHILD: &str = "PONKO_STARTUP_ERROR_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let expected = Error::new(ErrorKind::NotFound, "startup failed").with_path("level.json");
        let error = expected.clone();
        assert_eq!(
            Game::new("startup", 1, 1).try_setup(move |_| Err(error)).try_run(),
            Err(expected.clone())
        );
        struct Fails;
        impl Plugin for Fails {
            fn build(&self, game: Game) -> Game {
                game
            }
            fn try_setup(&self, _: &mut Ctx) -> Result<(), Error> {
                Err(Error::new(ErrorKind::NotFound, "startup failed").with_path("level.json"))
            }
        }
        assert_eq!(Game::new("startup", 1, 1).plugin(Fails).try_run(|_| {}), Err(expected));
        return;
    }
    // Isolate the documented smoke override from parallel tests' environment.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "smoke_runner_returns_setup_errors_without_a_window"])
        .env(CHILD, "1")
        .env("PONKO_SMOKE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
