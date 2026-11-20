use ponko::prelude::{Component, Ctx, Game, IntoSystem, Phase, Plugin, Systems};

#[derive(Component)]
struct Marker;
struct Area(&'static str);
impl Plugin for Area {
    fn build(&self, game: Game) -> Game {
        let name = self.0;
        game.add(Phase::BeforePhysics, move |ctx: &mut Ctx| {
            ctx.world.resource_mut::<Vec<&str>>().push(name)
        })
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.resource_mut::<Vec<&str>>().push(self.0);
        ctx.commands.spawn((Marker,));
    }
    // Added more than once, configured differently: each its own name.
    fn name(&self) -> &str {
        self.0
    }
}

#[test]
fn plugins_and_setups_interleave_and_apply_commands_before_systems() {
    let world = Game::new("plugins", 1, 1)
        .setup(|ctx| ctx.world.insert_resource(Vec::<&str>::new()))
        .plugin(Area("first"))
        .setup(|ctx| {
            assert_eq!(ctx.world.query::<&Marker>().count(), 1);
            ctx.world.resource_mut::<Vec<&str>>().push("user");
        })
        .plugin(Area("second"))
        .run_headless(1);
    assert_eq!(
        *world.resource::<Vec<&str>>(),
        ["first", "user", "second", "first", "second"]
    );
}

#[test]
fn plugin_setup_precedes_legacy_setup_and_runs_at_zero_ticks() {
    struct Init;
    impl Plugin for Init {
        fn build(&self, game: Game) -> Game {
            game
        }
        fn setup(&self, ctx: &mut Ctx) {
            ctx.commands.spawn((Marker,));
        }
    }
    let mut world = Game::new("plugins", 1, 1).plugin(Init).run_headless(0, |ctx| {
        assert_eq!(ctx.world.query::<&Marker>().count(), 1);
    });
    assert_eq!(world.query::<&Marker>().count(), 1);
}

#[test]
fn ids_distinguish_instances_of_the_same_closure_and_do_not_recycle() {
    let mut game = Game::new("ids", 1, 1);
    let add = |value| move |ctx: &mut Ctx| ctx.world.resource_mut::<Vec<i32>>().push(value);
    let first = game.add_system(Phase::AfterPhysics, add(1));
    let second = game.add_system(Phase::AfterPhysics, add(2));
    assert_ne!(first, second);
    let mut game = game.remove(first).remove(first);
    let third = game.add_system(Phase::AfterPhysics, add(3));
    assert_ne!(first, third);
    let world = game.run_headless(1, |ctx| ctx.world.insert_resource(Vec::<i32>::new()));
    assert_eq!(*world.resource::<Vec<i32>>(), [2, 3]);
}

#[test]
fn builtin_ids_remove_builtin_systems() {
    let game = Game::new("ids", 1, 1)
        .remove(Systems::DRAW_SPRITES)
        .remove(Systems::ANIMATE_SPRITES);
    assert_eq!(game.system_count(Systems::DRAW_SPRITES), 0);
    assert_eq!(game.system_count(Systems::ANIMATE_SPRITES), 0);
    game.run_headless(0, |_| {});
}

#[cfg(feature = "physics")]
#[test]
fn stable_physics_id_removes_step_after_repeated_configuration() {
    #[allow(unused_imports)]
    #[cfg(feature = "physics")]
    use ponko::d2::Collider;
    #[allow(unused_imports)]
    #[cfg(feature = "physics")]
    use ponko::d2::RigidBody;
    #[allow(unused_imports)]
    use ponko::d2::Transform as Transform2D;
    #[allow(unused_imports)]
    use ponko::input::Input;
    use ponko::prelude::*;
    let mut world = Game::new("physics ids", 1, 1)
        .physics(vec2(0.0, 980.0))
        .physics(vec2(0.0, 980.0))
        .remove(Systems::PHYSICS)
        .run_headless(60, |ctx| {
            ctx.world.spawn((
                Transform2D::at(vec2(0.0, 0.0)),
                RigidBody::dynamic(),
                Collider::circle(8.0),
            ));
        });
    assert_eq!(world.query::<&Transform2D>().iter().next().unwrap().position.y, 0.0);
}

#[test]
fn builtin_registrations_have_individual_handles_and_removal_by_handle_stays_exact() {
    let mut game = Game::new("ids", 1, 1);
    let first = game.add_system(Phase::Input, ponko::systems::rebuild_spatial_grid);
    let second = game.add_system(Phase::Input, ponko::systems::rebuild_spatial_grid);
    assert_ne!(first, second);
    let mut game = game.remove(first);
    // A second, renamed registration of a built-in counts under its constant
    // and is removed alone by its own handle.
    let custom = game.add_system(Phase::Frame, ponko::systems::draw_sprites.named("custom sprites"));
    assert_eq!(game.system_count(Systems::DRAW_SPRITES), 2);
    let game = game.remove(custom);
    assert_eq!(game.system_count(Systems::DRAW_SPRITES), 1);
    // The default registration stays removable by its constant.
    let game = game.remove(Systems::DRAW_SPRITES);
    assert_eq!(game.system_count(Systems::DRAW_SPRITES), 0);
    game.run_headless(0, |_| {});
}

#[test]
fn plugin_setup_is_shared_by_profiled_runs() {
    let (world, _) = Game::new("plugins", 1, 1)
        .setup(|ctx| ctx.world.insert_resource(Vec::<&str>::new()))
        .plugin(Area("profile"))
        .profile_headless(1, 2);
    assert_eq!(*world.resource::<Vec<&str>>(), ["profile"; 4]);
}

#[test]
fn user_ids_from_other_builders_are_harmless() {
    let mut first = Game::new("first", 1, 1);
    let foreign = first.add_system(Phase::Input, |_: &mut Ctx| {});
    let mut second = Game::new("second", 1, 1);
    let local = second.add_system(Phase::Input, |ctx: &mut Ctx| ctx.world.insert_resource(42usize));
    assert_ne!(foreign, local);
    let world = second.remove(foreign).run_headless(1, |_| {});
    assert_eq!(*world.resource::<usize>(), 42);
}

#[test]
fn plugin_systems_run_by_phase_not_by_plugin_order() {
    struct Late;
    impl Plugin for Late {
        fn build(&self, game: Game) -> Game {
            game.add(Phase::SimLast, |ctx: &mut Ctx| {
                ctx.world.resource_mut::<Vec<&str>>().push("sim last")
            })
            .add(Phase::AfterPhysics, |ctx: &mut Ctx| {
                ctx.world.resource_mut::<Vec<&str>>().push("after physics")
            })
        }
    }
    struct Early;
    impl Plugin for Early {
        fn build(&self, game: Game) -> Game {
            game.add(Phase::Input, |ctx: &mut Ctx| {
                ctx.world.resource_mut::<Vec<&str>>().push("input")
            })
            .add(Phase::BeforePhysics, |ctx: &mut Ctx| {
                ctx.world.resource_mut::<Vec<&str>>().push("before physics")
            })
        }
    }
    let world = Game::new("phases", 1, 1)
        .plugin(Late)
        .plugin(Early)
        .setup(|ctx| ctx.world.insert_resource(Vec::<&str>::new()))
        .run_headless(1);
    assert_eq!(
        *world.resource::<Vec<&str>>(),
        ["input", "before physics", "after physics", "sim last"]
    );
}

#[test]
fn removing_a_plugin_system_by_id_keeps_its_siblings() {
    struct Counter;
    impl Plugin for Counter {
        fn build(&self, mut game: Game) -> Game {
            let doomed = game.add_system(Phase::AfterPhysics, |ctx: &mut Ctx| {
                *ctx.world.resource_mut::<u32>() += 100
            });
            game.add(Phase::AfterPhysics, |ctx: &mut Ctx| {
                *ctx.world.resource_mut::<u32>() += 1
            })
            .remove(doomed)
        }
        fn setup(&self, ctx: &mut Ctx) {
            ctx.world.insert_resource(0u32);
        }
    }
    let world = Game::new("remove", 1, 1).plugin(Counter).run_headless(2, |_| {});
    assert_eq!(*world.resource::<u32>(), 2);
}

// ---------------------------------------------------------------- extension point

mod extension {
    use ponko::assets::MemorySource;
    use ponko::prelude::*;
    use ponko::reflect::{JsonValue, WorldReflect};
    use ponko::ui::{UiEvent, UiLayout, UiWidget};

    struct Base;
    impl Plugin for Base {
        fn build(&self, game: Game) -> Game {
            game.add(Phase::AfterPhysics, |ctx: &mut Ctx| {
                ctx.world.resource_mut::<Vec<&str>>().push("base")
            })
        }
        fn setup(&self, ctx: &mut Ctx) {
            ctx.world.insert_resource(vec!["base setup"]);
        }
    }

    struct Dependent;
    impl Plugin for Dependent {
        fn build(&self, game: Game) -> Game {
            game.add(Phase::AfterPhysics, |ctx: &mut Ctx| {
                ctx.world.resource_mut::<Vec<&str>>().push("dependent")
            })
        }
        fn setup(&self, ctx: &mut Ctx) {
            // The dependency's setup ran first: its resource is there.
            ctx.world.resource_mut::<Vec<&str>>().push("dependent setup");
        }
        fn dependencies(&self) -> Vec<&str> {
            vec![std::any::type_name::<Base>()]
        }
    }

    #[test]
    fn a_plugin_is_named_after_its_type_by_default() {
        assert_eq!(Base.name(), std::any::type_name::<Base>());
        assert!(Base.name().ends_with("::Base"));
        let game = Game::new("names", 1, 1).plugin(Base);
        assert!(game.has_plugin(std::any::type_name::<Base>()));
        assert!(!game.has_plugin(std::any::type_name::<Dependent>()));
    }

    #[test]
    fn adding_a_plugin_twice_fails_startup_and_builds_it_once() {
        let error = Game::new("twice", 1, 1)
            .plugin(Base)
            .plugin(Base)
            .try_run_headless(1, |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), ErrorKind::Invalid);
        assert!(error.message().contains("added twice"), "{error}");
        assert!(error.message().contains(std::any::type_name::<Base>()), "{error}");
    }

    #[test]
    #[should_panic(expected = "was added twice")]
    fn the_panicking_runners_report_a_duplicate_plugin() {
        Game::new("twice", 1, 1)
            .plugin(Base)
            .plugin(Base)
            .run_headless(0, |_| {});
    }

    #[test]
    fn a_failed_plugin_check_stops_startup_before_any_setup_runs() {
        let ran = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = ran.clone();
        let error = Game::new("first error", 1, 1)
            .setup(move |_| seen.set(true))
            .plugin(Dependent)
            .plugin(Base)
            .plugin(Base)
            .try_run_headless(0)
            .err()
            .unwrap();
        assert!(!ran.get(), "no setup runs once a plugin check failed");
        // The first problem is the one reported.
        assert!(error.message().contains("needs plugin"), "{error}");
    }

    #[test]
    fn a_missing_dependency_names_what_to_add_first() {
        let error = Game::new("missing", 1, 1)
            .plugin(Dependent)
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), ErrorKind::Invalid);
        let message = error.message();
        assert!(
            message.contains(std::any::type_name::<Dependent>())
                && message.contains(&format!("add `{}` first", std::any::type_name::<Base>())),
            "{message}"
        );
        // Added after its dependent is not "before it" either.
        let error = Game::new("late", 1, 1)
            .plugin(Dependent)
            .plugin(Base)
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert!(error.message().contains("wasn't added before it"), "{error}");
    }

    #[test]
    fn a_dependency_sets_up_and_runs_before_its_dependent_in_a_shared_phase() {
        let world = Game::new("order", 1, 1)
            .plugin(Base)
            .plugin(Dependent)
            .run_headless(1, |_| {});
        assert_eq!(
            *world.resource::<Vec<&str>>(),
            ["base setup", "dependent setup", "base", "dependent"]
        );
    }

    #[test]
    fn a_plugin_can_bring_its_dependency_unless_the_game_added_it() {
        struct Bundle;
        impl Plugin for Bundle {
            fn build(&self, game: Game) -> Game {
                let game = if game.has_plugin(std::any::type_name::<Base>()) {
                    game
                } else {
                    game.plugin(Base)
                };
                game.plugin(Dependent)
            }
        }
        for game in [
            Game::new("alone", 1, 1).plugin(Bundle),
            Game::new("with base", 1, 1).plugin(Base).plugin(Bundle),
        ] {
            let world = game.try_run_headless(1, |_| {}).unwrap();
            assert_eq!(world.resource::<Vec<&str>>().len(), 4);
        }
    }

    // A plugin as another crate would write it: its component and resource,
    // reflected; an asset source and a decoder; a widget; a debug toggle;
    // a control method. `tests/plugin-crate` builds the same outside the
    // workspace.

    #[derive(Component, serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct Cloud {
        rain: f32,
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct Weather {
        wind: f32,
    }

    /// A tiny made-up format: one cloud's rain per line.
    struct CloudFile(Vec<f32>);

    /// The plugin's loader: a decoder over `Assets::load_with`.
    fn load_clouds(assets: &ponko::assets::Assets, name: &str) -> Result<CloudFile, Error> {
        assets.load_with(name, |bytes| {
            let text = std::str::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::Decode, e.to_string()))?;
            text.lines()
                .map(|l| {
                    l.trim()
                        .parse()
                        .map_err(|_| Error::new(ErrorKind::Decode, format!("not a number: {l}")))
                })
                .collect::<Result<_, _>>()
                .map(CloudFile)
        })
    }

    /// The plugin's widget: a button that makes it rain.
    struct RainButton(ponko::ui::UiId);

    struct WeatherPlugin;
    impl Plugin for WeatherPlugin {
        fn build(&self, game: Game) -> Game {
            game.debug_toggle("weather.clouds", false)
                .control_method(
                    "weather.set_wind",
                    "sets the wind: params {\"wind\": number}",
                    |ctx: &mut Ctx, params: &JsonValue| {
                        let wind = params["wind"]
                            .as_f64()
                            .ok_or_else(|| Error::new(ErrorKind::Invalid, "`wind` must be a number"))?;
                        ctx.world.resource_mut::<Weather>().wind = wind as f32;
                        Ok(json!({ "wind": wind }))
                    },
                )
                .add(Phase::Frame, |ctx: &mut Ctx| {
                    let button = ctx.world.resource::<RainButton>().0;
                    let clicked = ctx.ui.events().any(|e| e == UiEvent::Clicked(button));
                    if clicked {
                        ctx.world.query::<&mut Cloud>().for_each(|c| c.rain += 1.0);
                    }
                })
                .add(Phase::Draw, |ctx: &mut Ctx| {
                    if ctx.debug.toggled("weather.clouds") {
                        ctx.debug.circle(vec2(0.0, 0.0), 10.0, Color::WHITE);
                    }
                })
        }
        fn try_setup(&self, ctx: &mut Ctx) -> Result<(), Error> {
            ctx.world.register::<Cloud>();
            ctx.world.reflect::<Cloud>();
            ctx.world.insert_resource(Weather { wind: 0.0 });
            ctx.world.reflect_resource::<Weather>();
            ctx.assets
                .add_source(MemorySource::new("weather").with("clouds.txt", "0.5\n2\n"));
            for rain in load_clouds(&ctx.assets, "clouds.txt")?.0 {
                ctx.world.spawn((Cloud { rain },));
            }
            let button = ctx.ui.add(UiWidget::Button, UiLayout::auto());
            ctx.ui.set_text(button, "Rain");
            ctx.ui.push_modeless(button);
            ctx.world.insert_resource(RainButton(button));
            Ok(())
        }
    }

    #[test]
    fn a_plugin_registers_reflected_types_assets_widgets_toggles_and_methods() {
        Game::new("weather", 1, 1)
            .debug_draw(true)
            .plugin(WeatherPlugin)
            .run_headless(0, |ctx| {
                // Components, resources and their reflection.
                let mut rains: Vec<f32> = ctx.world.query::<&Cloud>().iter().map(|c| c.rain).collect();
                rains.sort_by(f32::total_cmp);
                assert_eq!(rains, [0.5, 2.0], "spawned from the plugin's asset");
                let report: JsonValue = serde_json::from_str(&ctx.capabilities_json()).unwrap();
                let names = |kind: &str| -> Vec<String> {
                    report["reflection"][kind]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| c["name"].as_str().unwrap().to_string())
                        .collect()
                };
                assert!(names("components").iter().any(|n| n == "Cloud"), "{report}");
                assert!(names("resources").iter().any(|n| n == "Weather"), "{report}");
                // The plugin and its method are reported to tools.
                assert_eq!(report["plugins"], json!([std::any::type_name::<WeatherPlugin>()]));
                let method = report["methods"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["name"] == "weather.set_wind")
                    .unwrap();
                assert_eq!(
                    (method["added"].clone(), method["available"].clone()),
                    (json!(true), json!(true))
                );
                // The toggle starts off; turning it on draws.
                assert_eq!(ctx.debug.toggles().collect::<Vec<_>>(), [("weather.clouds", false)]);
                ctx.debug.set_toggle("weather.clouds", true).unwrap();
                assert!(ctx.debug.toggled("weather.clouds"));
                // The widget is on screen.
                let button = ctx.world.resource::<RainButton>().0;
                assert_eq!(ctx.ui.text(button), Some("Rain"));
                assert!(ctx.ui.is_shown(button));
            });
    }

    #[test]
    fn a_plugin_asset_decoder_error_stops_startup_with_the_assets_name() {
        struct BadClouds;
        impl Plugin for BadClouds {
            fn build(&self, game: Game) -> Game {
                game
            }
            fn try_setup(&self, ctx: &mut Ctx) -> Result<(), Error> {
                ctx.assets
                    .add_source(MemorySource::new("bad").with("clouds.txt", "rain\n"));
                load_clouds(&ctx.assets, "clouds.txt").map(|_| ())
            }
        }
        let error = Game::new("bad", 1, 1)
            .plugin(BadClouds)
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.message().contains("clouds.txt"), "{error}");
    }

    #[test]
    fn debug_toggles_and_control_methods_are_added_once() {
        let error = Game::new("toggles", 1, 1)
            .debug_toggle("a.b", false)
            .debug_toggle("a.b", true)
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert!(
            error.message().contains("debug toggle `a.b` was added twice"),
            "{error}"
        );
        let none = |_: &mut Ctx, _: &JsonValue| Ok(JsonValue::Null);
        let error = Game::new("methods", 1, 1)
            .control_method("arena.ping", "ping", none)
            .control_method("arena.ping", "ping", none)
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert!(error.message().contains("added twice"), "{error}");
        let error = Game::new("methods", 1, 1)
            .control_method("entity.ping", "ping", none)
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert!(error.message().contains("engine's namespaces"), "{error}");
        let error = (0..65)
            .fold(Game::new("many", 1, 1), |game, i| {
                game.debug_toggle(format!("t.{i}"), false)
            })
            .try_run_headless(0, |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), ErrorKind::Limit);
    }

    #[test]
    fn a_debug_toggle_follows_debug_drawing() {
        Game::new("off", 1, 1)
            .debug_draw(false)
            .debug_toggle("a.b", true)
            .run_headless(0, |ctx| {
                assert!(!ctx.debug.toggled("a.b"), "debug drawing is off");
                ctx.debug.set_enabled(true);
                assert!(ctx.debug.toggled("a.b"));
                assert!(!ctx.debug.toggled("a.unknown"));
            });
    }

    #[test]
    fn a_debug_toggle_reaches_view_systems_in_parallel_batches() {
        use ponko::access::SystemAccess;
        use ponko::system_view::SystemView;
        #[derive(Component)]
        struct A;
        #[derive(Component)]
        struct B;
        fn draw(sys: &mut SystemView) {
            if sys.debug.toggled("a.b") {
                sys.debug.line(vec2(0.0, 0.0), vec2(1.0, 0.0), Color::RED);
            }
        }
        let world = Game::new("views", 1, 1)
            .debug_draw(true)
            .debug_toggle("a.b", true)
            .parallel_systems(true)
            .parallel_threshold(std::time::Duration::ZERO)
            .add(Phase::AfterPhysics, draw.access(SystemAccess::new().write::<A>()))
            .add(Phase::AfterPhysics, draw.access(SystemAccess::new().write::<B>()))
            .add(Phase::SimLast, |ctx: &mut Ctx| {
                let lines = ctx.debug.stats().lines_2d;
                ctx.world.insert_resource(lines);
            })
            .run_headless(1, |_| {});
        assert_eq!(*world.resource::<usize>(), 2, "both view systems saw the toggle on");
    }
}
