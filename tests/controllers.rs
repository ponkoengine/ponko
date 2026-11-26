//! Controllers through the fake gamepad backend: identity and families,
//! rumble, motion, hot-plug slots, players, the last used device and the
//! prompts that follow it. Everything runs headless; what reaches
//! `FakeGamepads` is what a real backend would be told.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use ponko::actions::InputEvent;
use ponko::input::{
    ControllerFamily, DeviceKey, FakeGamepads, Gamepad, GamepadAxis, GamepadButton, GamepadCapabilities, GamepadInfo,
    GamepadMotion, GamepadStick, Glyph, InputDevice, MotorLevels, Rumble,
};
use ponko::prelude::*;
use ponko::scenario::InputScenario;

/// Runs `frames` headless frames reading pads from `fake`; `each`
/// runs in a simulation system with the frame number (0 first). What the
/// fake is told during frame `n` reaches `ctx.input` in frame `n + 1`.
fn run(fake: &FakeGamepads, frames: u64, mut each: impl FnMut(&mut Ctx, u64) + 'static) {
    let mut frame = 0;
    Game::new("controllers", 1, 1)
        .gamepad_backend(fake.clone())
        .sim_system(move |ctx: &mut Ctx| {
            each(ctx, frame);
            frame += 1;
        })
        .run_headless(frames, |_| {});
}

fn pad(slot: usize) -> Gamepad {
    Gamepad::from_slot(slot).unwrap()
}

fn rumbling() -> GamepadInfo {
    GamepadInfo::new("Pad").capabilities(GamepadCapabilities {
        rumble: true,
        trigger_rumble: true,
        motion: true,
        ..Default::default()
    })
}

#[test]
fn a_connected_pad_reports_its_name_ids_family_and_capabilities() {
    let fake = FakeGamepads::new();
    fake.connect(rumbling().ids(0x054c, 0x0ce6));
    run(&fake, 1, |ctx, _| {
        assert_eq!(ctx.input.gamepads(), [pad(0)]);
        let info = ctx.input.gamepad_info(pad(0)).unwrap();
        assert_eq!(info.name, "Pad");
        assert_eq!((info.vendor_id, info.product_id), (Some(0x054c), Some(0x0ce6)));
        assert_eq!(info.family, ControllerFamily::PlayStation);
        assert!(info.capabilities.rumble && info.capabilities.motion);
        assert_eq!(ctx.input.gamepads_connected().collect::<Vec<_>>(), [pad(0)]);
    });
}

#[test]
fn families_are_detected_from_vendor_ids_and_names() {
    let detect = ControllerFamily::detect;
    assert_eq!(detect(Some(0x045e), Some(0x02ea), ""), ControllerFamily::Xbox);
    assert_eq!(
        detect(Some(0x054c), None, "Wireless Controller"),
        ControllerFamily::PlayStation
    );
    assert_eq!(detect(Some(0x057e), None, ""), ControllerFamily::Nintendo);
    assert_eq!(detect(None, None, "Xbox Controller"), ControllerFamily::Xbox);
    assert_eq!(detect(None, None, "PS4 Controller"), ControllerFamily::PlayStation);
    assert_eq!(detect(None, None, "Joy-Con (L/R)"), ControllerFamily::Nintendo);
    assert_eq!(detect(Some(0x1234), None, "USB Gamepad"), ControllerFamily::Generic);
}

#[test]
fn the_family_can_be_overridden_until_the_pad_leaves() {
    let fake = FakeGamepads::new();
    let device = fake.connect(GamepadInfo::new("Xbox Controller"));
    let script = fake.clone();
    run(&fake, 4, move |ctx, frame| match frame {
        0 => {
            assert_eq!(ctx.input.gamepad_family(pad(0)), ControllerFamily::Xbox);
            ctx.input.set_gamepad_family(pad(0), ControllerFamily::Nintendo);
            assert_eq!(ctx.input.gamepad_family(pad(0)), ControllerFamily::Nintendo);
            script.disconnect(device);
            script.reconnect(device);
        }
        1 => assert_eq!(
            ctx.input.gamepad_family(pad(0)),
            ControllerFamily::Xbox,
            "detected again"
        ),
        _ => {}
    });
}

#[test]
fn rumble_reaches_the_backend_and_stops_after_its_time() {
    let fake = FakeGamepads::new();
    let device = fake.connect(rumbling());
    let script = fake.clone();
    // 60 Hz frames: 0.1 s is six frames.
    run(&fake, 12, move |ctx, frame| {
        if frame == 0 {
            ctx.input.rumble(pad(0), Rumble::new(0.8, 0.4).seconds(0.1));
            assert_eq!(ctx.input.rumble_levels(pad(0)).strong, 0.8);
        }
        if frame == 2 {
            let sent = script.rumble(device);
            assert_eq!((sent.strong, sent.weak), (0.8, 0.4), "sent at the next frame's start");
        }
        if frame == 11 {
            assert!(ctx.input.rumble_levels(pad(0)).is_off());
            assert!(script.rumble(device).is_off(), "stopped after 0.1 s");
            assert_eq!(script.rumble_changes(device), 2, "one start, one stop: not every frame");
        }
    });
}

#[test]
fn trigger_motors_reach_the_backend_and_values_are_clamped() {
    let fake = FakeGamepads::new();
    let device = fake.connect(rumbling());
    run(&fake, 2, |ctx, frame| {
        if frame == 0 {
            ctx.input.rumble(
                pad(0),
                Rumble::new(2.0, f32::NAN).triggers(-1.0, 0.7).seconds(f32::INFINITY),
            );
        }
    });
    assert_eq!(
        fake.rumble(device),
        MotorLevels {
            strong: 1.0,
            weak: 0.0,
            left_trigger: 0.0,
            right_trigger: 0.7
        }
    );
}

#[test]
fn stop_rumble_and_focus_loss_turn_the_motors_off() {
    let fake = FakeGamepads::new();
    let device = fake.connect(rumbling());
    let script = fake.clone();
    run(&fake, 6, move |ctx, frame| match frame {
        0 => ctx.input.rumble(pad(0), Rumble::new(1.0, 1.0).seconds(60.0)),
        2 => {
            assert!(!script.rumble(device).is_off());
            ctx.input.stop_rumble(pad(0));
        }
        3 => {
            assert!(script.rumble(device).is_off(), "stopped");
            ctx.input.rumble(pad(0), Rumble::new(1.0, 1.0).seconds(60.0));
        }
        4 => ctx.input.feed(InputEvent::FocusLost),
        5 => assert!(script.rumble(device).is_off(), "focus loss stops rumble"),
        _ => {}
    });
}

#[test]
fn rumble_on_an_empty_slot_does_nothing() {
    let fake = FakeGamepads::new();
    run(&fake, 2, |ctx, _| {
        ctx.input.rumble(pad(3), Rumble::new(1.0, 1.0));
        assert!(ctx.input.rumble_levels(pad(3)).is_off());
    });
}

#[test]
fn a_reconnected_pad_gets_its_old_slot_back() {
    let fake = FakeGamepads::new();
    let a = fake.connect(GamepadInfo::new("A"));
    let _b = fake.connect(GamepadInfo::new("B"));
    let script = fake.clone();
    run(&fake, 4, move |ctx, frame| match frame {
        0 => {
            assert_eq!(ctx.input.gamepads(), [pad(0), pad(1)]);
            script.disconnect(a);
            script.connect(GamepadInfo::new("C"));
        }
        1 => {
            assert_eq!(ctx.input.gamepads_disconnected().collect::<Vec<_>>(), [pad(0)]);
            assert_eq!(ctx.input.gamepads_connected().collect::<Vec<_>>(), [pad(2)]);
            assert_eq!(ctx.input.gamepads(), [pad(1), pad(2)], "C skips the slot A left");
            script.reconnect(a);
        }
        2 => {
            assert_eq!(ctx.input.gamepads(), [pad(0), pad(1), pad(2)]);
            assert_eq!(ctx.input.gamepad_info(pad(0)).unwrap().name, "A", "A is back in slot 0");
        }
        _ => {}
    });
}

#[test]
fn a_new_pad_takes_a_waiting_slot_when_no_other_is_free() {
    let fake = FakeGamepads::new();
    let keys: Vec<DeviceKey> = (0..ponko::input::MAX_GAMEPADS)
        .map(|i| fake.connect(GamepadInfo::new(format!("{i}"))))
        .collect();
    let script = fake.clone();
    run(&fake, 3, move |ctx, frame| match frame {
        0 => {
            assert_eq!(ctx.input.gamepads().len(), ponko::input::MAX_GAMEPADS);
            script.disconnect(keys[3]);
            script.connect(GamepadInfo::new("new"));
        }
        1 => assert_eq!(ctx.input.gamepad_info(pad(3)).unwrap().name, "new"),
        _ => {}
    });
}

#[test]
fn unplugging_releases_held_buttons_this_frame() {
    let fake = FakeGamepads::new();
    let device = fake.connect(GamepadInfo::new("Pad"));
    let script = fake.clone();
    run(&fake, 3, move |ctx, frame| match frame {
        0 => script.press(device, GamepadButton::South),
        1 => {
            assert!(ctx.input.gamepad_down(pad(0), GamepadButton::South));
            script.disconnect(device);
        }
        2 => {
            assert!(ctx.input.gamepad_released(pad(0), GamepadButton::South));
            assert!(ctx.input.gamepads().is_empty());
            assert!(ctx.input.gamepad_info(pad(0)).is_none());
        }
        _ => {}
    });
}

#[test]
fn players_keep_their_pad_while_it_is_unplugged_and_back() {
    let fake = FakeGamepads::new();
    let first = fake.connect(GamepadInfo::new("One"));
    let second = fake.connect(GamepadInfo::new("Two"));
    let script = fake.clone();
    run(&fake, 5, move |ctx, frame| match frame {
        0 => {
            script.press(second, GamepadButton::Start);
        }
        1 => {
            let joined = ctx.input.join_pressed(GamepadButton::Start).unwrap();
            assert_eq!(joined, pad(1));
            let player = ctx.input.free_player().unwrap();
            assert_eq!(player, 0);
            assert!(ctx.input.assign_player(player, InputDevice::Gamepad(joined)));
            assert!(ctx.input.assign_player(1, InputDevice::KeyboardMouse));
            assert_eq!(ctx.input.join_pressed(GamepadButton::Start), None, "already a player");
            script.release(second, GamepadButton::Start);
            script.disconnect(second);
        }
        2 => {
            assert!(!ctx.input.player_connected(0), "its pad is unplugged");
            assert_eq!(ctx.input.player_device(0), Some(InputDevice::Gamepad(pad(1))));
            assert!(ctx.input.player_connected(1), "the keyboard is always there");
            script.reconnect(second);
        }
        3 => {
            assert!(ctx.input.player_connected(0), "back in its slot");
            let _ = first;
            ctx.input.unassign_player(0);
            assert_eq!(ctx.input.player_device(0), None);
            assert!(
                !ctx.input
                    .assign_player(ponko::input::MAX_PLAYERS, InputDevice::KeyboardMouse)
            );
        }
        _ => {}
    });
}

#[test]
fn motion_arrives_from_backends_that_read_it() {
    let fake = FakeGamepads::new();
    let device = fake.connect(rumbling());
    let script = fake.clone();
    let reading = GamepadMotion {
        gyro: vec3(0.0, -2.0, 0.0),
        accel: vec3(0.0, 9.81, 0.0),
    };
    run(&fake, 3, move |ctx, frame| match frame {
        0 => {
            assert_eq!(ctx.input.gamepad_motion(pad(0)), None, "none before the first reading");
            script.set_motion(device, reading);
        }
        1 => {
            let motion = ctx.input.gamepad_motion(pad(0)).unwrap();
            assert_eq!(motion, reading);
            assert!(motion.aim_delta(0.5).x > 0.0, "turning right aims right");
        }
        _ => {}
    });
}

#[test]
fn the_last_device_follows_presses_not_stick_drift() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        let p = pad(0);
        assert_eq!(ctx.input.last_device(), InputDevice::KeyboardMouse);
        ctx.input.feed(InputEvent::GamepadAxis(p, GamepadAxis::LeftStickX, 0.2));
        assert_eq!(
            ctx.input.last_device(),
            InputDevice::KeyboardMouse,
            "drift under halfway"
        );
        ctx.input.feed(InputEvent::GamepadAxis(p, GamepadAxis::LeftStickX, 0.9));
        assert_eq!(ctx.input.last_device(), InputDevice::Gamepad(p));
        assert!(ctx.input.last_device_changed());
        ctx.input.feed(InputEvent::MouseMove(vec2(1.0, 1.0)));
        assert_eq!(ctx.input.last_device(), InputDevice::Gamepad(p), "a nudged mouse");
        ctx.input.feed(InputEvent::MouseMove(vec2(40.0, 1.0)));
        assert_eq!(ctx.input.last_device(), InputDevice::KeyboardMouse);
        ctx.input
            .feed(InputEvent::GamepadButton(pad(1), GamepadButton::North, true));
        assert_eq!(ctx.input.last_device(), InputDevice::Gamepad(pad(1)));
        ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
        assert_eq!(ctx.input.last_device(), InputDevice::KeyboardMouse);
    });
}

fn prompt_map(ctx: &mut Ctx) {
    let p = pad(0);
    let mut map = ActionMap::default();
    map.set(
        "jump",
        Action::Button(vec![
            ButtonBinding::Key(Key::Space),
            ButtonBinding::Gamepad {
                pad: p,
                button: GamepadButton::South,
            },
        ]),
    );
    map.set(
        "fire",
        Action::Button(vec![
            ButtonBinding::Mouse(MouseButton::Left),
            ButtonBinding::Gamepad {
                pad: p,
                button: GamepadButton::RightTrigger,
            },
        ]),
    );
    map.set(
        "move",
        Action::Axis2(vec![
            Axis2Binding::Keys {
                left: Key::A,
                right: Key::D,
                up: Key::W,
                down: Key::S,
            },
            Axis2Binding::Gamepad {
                pad: p,
                stick: GamepadStick::Left,
                deadzone: 0.2,
            },
        ]),
    );
    map.set(
        "steer",
        Action::Axis(vec![AxisBinding::Gamepad {
            pad: p,
            axis: GamepadAxis::LeftStickX,
            deadzone: 0.2,
        }]),
    );
    ctx.input.set_actions(map);
}

#[test]
fn prompts_follow_the_last_used_device_and_its_family() {
    let fake = FakeGamepads::new();
    let xbox = fake.connect(GamepadInfo::new("Xbox Wireless Controller"));
    let ps = fake.connect(GamepadInfo::new("DualSense Wireless Controller"));
    let script = fake.clone();
    run(&fake, 4, move |ctx, frame| match frame {
        0 => {
            prompt_map(ctx);
            assert_eq!(ctx.input.action_glyphs("jump").to_string(), "Space");
            assert_eq!(ctx.input.action_glyphs("fire").to_string(), "Left Mouse");
            assert_eq!(ctx.input.action_glyphs("move").to_string(), "W/A/S/D");
            assert!(ctx.input.action_glyphs("steer").is_empty(), "no keyboard binding");
            assert!(ctx.input.action_glyphs("nope").is_empty());
            script.press(xbox, GamepadButton::Start);
        }
        1 => {
            assert!(ctx.input.last_device_changed());
            let jump = ctx.input.action_glyphs("jump");
            assert_eq!(
                jump.first(),
                Some(Glyph::Button(ControllerFamily::Xbox, GamepadButton::South))
            );
            assert_eq!(jump.to_string(), "A");
            assert_eq!(ctx.input.action_glyphs("fire").to_string(), "RT");
            assert_eq!(
                ctx.input.action_glyphs("move").first(),
                Some(Glyph::Stick(ControllerFamily::Xbox, GamepadStick::Left))
            );
            assert_eq!(ctx.input.action_glyphs("steer").to_string(), "Left Stick X");
            script.press(ps, GamepadButton::Start);
        }
        2 => {
            // The map binds slot 0 only; the PlayStation pad in slot 1 still
            // gets its own labels for those buttons.
            assert_eq!(ctx.input.last_device(), InputDevice::Gamepad(pad(1)));
            assert_eq!(ctx.input.action_glyphs("jump").to_string(), "Cross");
            assert_eq!(ctx.input.action_glyphs("fire").to_string(), "R2");
            let for_xbox = ctx.input.action_glyphs_for("jump", InputDevice::Gamepad(pad(0)));
            assert_eq!(for_xbox.to_string(), "A", "a player's own prompts");
        }
        _ => {}
    });
}

#[test]
fn key_prompts_follow_the_keyboard_layout_in_text_atlas_caps_and_inline_images() {
    use ponko::input::{GlyphAtlas, KeyLabels};
    // A French (AZERTY) keyboard: the keys in W and A's places read Z and Q.
    let azerty = KeyLabels::from_layout(|key| match key {
        Key::W => Some('z'),
        Key::A => Some('q'),
        Key::Q => Some('a'),
        Key::Z => Some('w'),
        _ => None,
    });
    let us_atlas = GlyphAtlas::new(32);
    let world = Game::new("t", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            let mut atlas = GlyphAtlas::new(32);
            match ctx.time.sim_tick() {
                0 => {
                    prompt_map(ctx);
                    let wasd = ctx.input.action_glyphs("move");
                    assert_eq!(
                        ctx.input.key_labels().text(&wasd),
                        "W/A/S/D",
                        "a US keyboard until told"
                    );
                    ctx.input.set_key_labels(azerty.clone());
                    assert!(ctx.input.key_labels_changed());
                }
                1 => {
                    assert!(!ctx.input.key_labels_changed(), "an edge, for one frame");
                    let wasd = ctx.input.action_glyphs("move");
                    assert_eq!(
                        wasd.first(),
                        Some(Glyph::Key(Key::W)),
                        "bindings stay physical: the same key"
                    );
                    assert_eq!(ctx.input.key_labels().text(&wasd), "Z/Q/S/D");
                    assert_eq!(wasd.to_string(), "W/A/S/D", "Glyph::label is the US position's");
                    assert_eq!(ctx.input.key_labels().text(&ctx.input.action_glyphs("jump")), "Space");
                    atlas.set_key_labels(ctx.input.key_labels());
                    assert_eq!(atlas.key_labels(), ctx.input.key_labels());
                    let cap = |a: &GlyphAtlas, k| a.glyph_rgba(Glyph::Key(k));
                    assert_ne!(cap(&atlas, Key::W), cap(&us_atlas, Key::W), "W's cap reads Z");
                    assert_eq!(cap(&atlas, Key::W), cap(&us_atlas, Key::Z), "drawn as the US Z cap");
                    assert_eq!(cap(&atlas, Key::S), cap(&us_atlas, Key::S), "S is S on both");
                    let markup = atlas.inline_image(&mut ctx.gfx, &mut ctx.ui, Glyph::Key(Key::W));
                    assert_eq!(markup, "[img=key.w alt=Z]");
                    ctx.world.insert_resource(true);
                }
                _ => {}
            }
        })
        .run_headless(2, |_| {});
    assert!(world.get_resource::<bool>().is_some(), "the checks ran");
}

#[test]
fn per_player_actions_retarget_gamepad_bindings() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        prompt_map(ctx);
        let jump = ctx.input.actions().get("jump").unwrap().for_pad(pad(1));
        ctx.input.set_action("jump.p2", jump);
        ctx.input
            .feed(InputEvent::GamepadButton(pad(1), GamepadButton::South, true));
        assert!(ctx.input.action_down("jump.p2"));
        assert!(!ctx.input.action_down("jump"), "player one's binding is slot 0");
    });
}

#[test]
fn feeding_connects_and_motion_drive_a_game_without_a_backend() {
    let scenario = InputScenario::from_json(
        r#"{"version":1,"duration":3,"steps":[
            {"tick":0,"type":"gamepad_connected","pad":1},
            {"tick":1,"type":"gamepad_motion","pad":1,"gyro":[0.0,1.0,0.0],"accel":[0.0,9.81,0.0]}
        ]}"#,
    )
    .unwrap();
    let mut frame = 0;
    let (_, report) = Game::new("t", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            if frame >= 1 {
                assert_eq!(ctx.input.gamepads(), [pad(1)]);
                assert_eq!(ctx.input.gamepad_family(pad(1)), ControllerFamily::Generic);
            }
            if frame == 1 {
                assert_eq!(ctx.input.gamepad_motion(pad(1)).unwrap().gyro, vec3(0.0, 1.0, 0.0));
            }
            frame += 1;
        })
        .run_headless_scenario(scenario, |_| {});
    report.assert_passed();
}

// ------------------------------------------------- steady state allocates nothing

struct CountingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation and deallocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn polling_rumble_motion_and_prompts_allocate_nothing_per_frame() {
    let fake = FakeGamepads::new();
    fake.connect(rumbling());
    run(&fake, 6, |ctx, frame| {
        if frame == 0 {
            prompt_map(ctx);
            ctx.input.rumble(pad(0), Rumble::new(0.5, 0.5).seconds(60.0));
        }
        if frame == 2 {
            ALLOCS.set(0);
            TRACK.set(true);
        }
        // Tracking stays on across the frame start, where the backend is polled.
        for _ in 0..16 {
            std::hint::black_box(ctx.input.action_glyphs("jump"));
            std::hint::black_box(ctx.input.action_glyphs_for("move", InputDevice::Gamepad(pad(0))));
            std::hint::black_box(ctx.input.gamepad_motion(pad(0)));
            std::hint::black_box(ctx.input.rumble_levels(pad(0)));
            std::hint::black_box(ctx.input.last_device());
            std::hint::black_box(ctx.input.join_pressed(GamepadButton::Start));
        }
        if frame == 5 {
            TRACK.set(false);
            assert_eq!(ALLOCS.get(), 0);
        }
    });
}
