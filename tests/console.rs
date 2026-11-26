use ponko::console::{ConsoleArg, ConsoleType, ConsoleValue};
use ponko::prelude::*;

// ANCHOR: developer_console
fn add_console(game: Game) -> Game {
    game.console_variable("show_paths", "Show path debug lines", ConsoleValue::Bool(false))
        .console_command(
            "echo",
            "Print a count",
            &[ConsoleArg::new("count", ConsoleType::Integer)],
            |_, args| match args {
                [ConsoleValue::Integer(count)] => Ok(format!("Count: {count}")),
                _ => Err(Error::new(ErrorKind::Invalid, "expected a count")),
            },
        )
}
// ANCHOR_END: developer_console

#[test]
fn the_cookbook_console_registers_an_integer_command() {
    add_console(Game::new("Console", 1, 1))
        .developer_console(true)
        .run_headless(0, |ctx| {
            assert_eq!(ctx.run_console("echo 42").unwrap(), "Count: 42");
            assert_eq!(ctx.run_console("show_paths true").unwrap(), "true");
        });
}

#[test]
fn commands_receive_validated_arguments_and_variables_keep_their_type() {
    Game::new("Console", 640, 480)
        .developer_console(true)
        .console_command(
            "give",
            "Give an item",
            &[
                ConsoleArg::new("item", ConsoleType::Text),
                ConsoleArg::new("count", ConsoleType::Integer),
            ],
            |ctx, args| {
                let [ConsoleValue::Text(item), ConsoleValue::Integer(count)] = args else {
                    unreachable!("arguments are validated before dispatch");
                };
                ctx.world.insert_resource((item.clone(), *count));
                Ok(format!("Gave {count} × {item}"))
            },
        )
        .console_variable("show_paths", "Show path debug lines", ConsoleValue::Bool(false))
        .run_headless(0, |ctx| {
            let help = ctx.run_console("help").unwrap();
            assert!(help.contains("give <item:text> <count:integer>"));
            assert!(help.contains("show_paths [value:bool]"));
            assert!(ctx.run_console("give stone").unwrap_err().message().contains("count"));
            let error = ctx.run_console("give stone many").unwrap_err();
            assert!(error.message().contains("count"));
            assert!(error.message().contains("integer"));
            assert!(ctx.world.get_resource::<(String, i64)>().is_none());
            assert_eq!(ctx.run_console(r#"give "red gem" 2"#).unwrap(), "Gave 2 × red gem");
            assert_eq!(ctx.world.resource::<(String, i64)>().0, "red gem");
            assert_eq!(ctx.run_console("show_paths").unwrap(), "false");
            assert_eq!(ctx.run_console("show_paths true").unwrap(), "true");
            assert!(ctx.run_console("show_paths 123").is_err());
            assert_eq!(ctx.console_value("show_paths"), Some(&ConsoleValue::Bool(true)));
        });
}

#[test]
fn malformed_or_duplicate_registrations_fail_before_setup() {
    let game = Game::new("Console", 1, 1)
        .console_variable("level", "Level", ConsoleValue::Integer(1))
        .console_command("level", "Get level", &[], |_, _| Ok(String::new()));
    let error = game
        .try_run_headless(0, |_| -> () { panic!("startup must fail") })
        .err()
        .unwrap();
    assert!(error.message().contains("level"));
    assert!(error.message().contains("already registered"));
}

#[test]
fn disabling_the_console_blocks_execution_and_preserves_variable_defaults() {
    Game::new("Console", 1, 1)
        .developer_console(false)
        .console_variable("gravity", "Gravity scale", ConsoleValue::Float(1.0))
        .run_headless(0, |ctx| {
            assert!(ctx.run_console("gravity 0").unwrap_err().message().contains("disabled"));
            assert_eq!(ctx.console_value("gravity"), Some(&ConsoleValue::Float(1.0)));
        });
}

#[test]
fn retained_overlay_opens_completes_submits_and_recalls_input_without_a_window() {
    use ponko::actions::InputEvent;
    let tap = |ctx: &mut Ctx, key| {
        ctx.input.feed(InputEvent::Key(key, true));
        ctx.input.feed(InputEvent::Key(key, false));
    };
    Game::new("Console", 640, 480)
        .headless_frames(true)
        .developer_console(true)
        .console_key(Key::F8)
        .console_command("ping", "Respond with pong", &[], |_, _| Ok("pong".into()))
        .system(move |ctx: &mut Ctx| {
            let input = || ctx.ui.find("ponko.console.input").unwrap();
            match ctx.time.sim_tick() {
                1 => {
                    assert_eq!(ctx.ui.focus(), Some(input()));
                    ctx.input.insert_text("pi");
                    tap(ctx, Key::Tab);
                }
                2 => {
                    assert_eq!(ctx.ui.text(input()), Some("ping "));
                    tap(ctx, Key::Enter);
                }
                3 => {
                    let log = ctx.ui.find("ponko.console.output").unwrap();
                    assert!(ctx.ui.text(log).unwrap().contains("> ping\npong"));
                    tap(ctx, Key::Up);
                }
                4 => {
                    assert_eq!(ctx.ui.text(input()), Some("ping"));
                    tap(ctx, Key::Down);
                }
                5 => {
                    assert_eq!(ctx.ui.text(input()), Some(""));
                    tap(ctx, Key::Escape);
                }
                6 => assert!(ctx.ui.top_screen().is_none()),
                _ => {}
            }
        })
        .run_headless(6, |ctx| tap(ctx, Key::F8));
}

#[test]
fn parser_preserves_empty_unicode_and_escaped_text_arguments() {
    Game::new("Console", 1, 1)
        .developer_console(true)
        .console_variable("message", "A message", ConsoleValue::Text("Ready".into()))
        .run_headless(0, |ctx| {
            assert_eq!(ctx.run_console(r#"message 'résumé 工作'"#).unwrap(), "résumé 工作");
            assert_eq!(ctx.run_console(r#"message Résumé\ 1"#).unwrap(), "Résumé 1");
            assert_eq!(ctx.run_console(r#"message "say \"hello\"""#).unwrap(), "say \"hello\"");
            let error = ctx.run_console(r#"message "unclosed"#).unwrap_err();
            assert!(error.message().contains("unclosed"));
            assert!(
                ctx.run_console("message end\\")
                    .unwrap_err()
                    .message()
                    .contains("escape")
            );
            assert_eq!(ctx.run_console("message").unwrap(), "say \"hello\"");
            assert_eq!(ctx.run_console(r#"message """#).unwrap(), "");
        });
}

#[test]
fn registration_enables_the_console_by_default_only_in_debug_builds() {
    Game::new("Console", 1, 1)
        .console_variable("scale", "Scale", ConsoleValue::Float(1.0))
        .run_headless(0, |ctx| {
            assert_eq!(ctx.run_console("help").is_ok(), cfg!(debug_assertions));
            assert_eq!(ctx.console_value("scale"), Some(&ConsoleValue::Float(1.0)));
        });
}
