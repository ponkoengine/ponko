//! Rumble in the browser, for both web gamepad backends: the ordinary build's
//! (gilrs, which has no force feedback on the web) and the threaded build's
//! (`gamepad_web`).
//!
//! A browser plays a pad's rumble through `gamepad.vibrationActuator`:
//! `playEffect("dual-rumble", {startDelay, duration, weakMagnitude,
//! strongMagnitude})`. An effect ends by itself after its `duration`, and
//! browsers cap that at 5 seconds (Chromium), while the engine's rumble can
//! run longer (`Rumble::seconds`, up to `f32::INFINITY`). So [`WebRumble`]
//! plays short effects and plays the same levels again every
//! [`RESEND`], well before the cap, until the engine says off. A game that
//! stalls (a hidden tab, a long frame) stops buzzing by itself within
//! [`EFFECT_MS`] instead of at the cap.
//!
//! [`WebRumble`] is plain Rust over the [`Actuators`] trait and is tested
//! natively with a fake; the page side is [`Browser`], which keeps no
//! browser object (it looks the pad up in `navigator.getGamepads()` each
//! time), so it is as good in a build with threads as in one without.
//!
//! The browser's actuators drive the two body motors only: trigger motors
//! (`trigger-rumble`) are not used, and the capability stays `false`.

use web_time::{Duration, Instant};

use crate::input::{DeviceKey, MotorLevels};

/// How long each effect asked of the browser lasts: well under its 5 s cap.
pub(crate) const EFFECT_MS: u32 = 2000;
/// How long a playing effect is left before the same levels are played
/// again: half of [`EFFECT_MS`], so a late frame never leaves a gap.
pub(crate) const RESEND: Duration = Duration::from_millis(1000);

/// What the browser is asked to do with a pad's `vibrationActuator`.
pub(crate) trait Actuators {
    /// The clock the re-sending counts on (a test's replaces it).
    fn now(&self) -> Instant {
        Instant::now()
    }
    /// `playEffect("dual-rumble", ...)` on the pad at browser `index`; a
    /// new effect replaces the one playing.
    fn play(&mut self, index: u32, strong: f32, weak: f32, duration_ms: u32);
    /// `reset()`: stops whatever plays on that pad.
    fn stop(&mut self, index: u32);
}

/// A pad that is rumbling.
#[derive(Debug, Clone, Copy)]
struct Playing {
    device: DeviceKey,
    /// Its place in `navigator.getGamepads()`.
    index: u32,
    levels: MotorLevels,
    sent: Instant,
}

/// The rumble playing on each pad, kept alive past the browser's cap.
#[derive(Debug, Default)]
pub(crate) struct WebRumble {
    playing: Vec<Playing>,
}

impl WebRumble {
    /// The engine's wish for `device` (browser pad `index`): plays the body
    /// motors' levels now, or stops the pad when they are off.
    pub(crate) fn set(&mut self, actuators: &mut impl Actuators, device: DeviceKey, index: u32, levels: MotorLevels) {
        let was = self.playing.iter().position(|p| p.device == device);
        if levels.strong <= 0.0 && levels.weak <= 0.0 {
            if let Some(at) = was {
                let old = self.playing.swap_remove(at);
                actuators.stop(old.index);
            }
            return;
        }
        let now = actuators.now();
        let playing = Playing {
            device,
            index,
            levels,
            sent: now,
        };
        match was {
            Some(at) => {
                let old = std::mem::replace(&mut self.playing[at], playing);
                if old.index != index {
                    actuators.stop(old.index);
                }
            }
            None => self.playing.push(playing),
        }
        actuators.play(index, levels.strong, levels.weak, EFFECT_MS);
    }

    /// Once per frame: plays again what was last sent a [`RESEND`] ago.
    pub(crate) fn refresh(&mut self, actuators: &mut impl Actuators) {
        if self.playing.is_empty() {
            return;
        }
        let now = actuators.now();
        for p in &mut self.playing {
            if now.saturating_duration_since(p.sent) >= RESEND {
                p.sent = now;
                actuators.play(p.index, p.levels.strong, p.levels.weak, EFFECT_MS);
            }
        }
    }

    /// The pad is gone: forgets it, with no call (the browser has dropped
    /// its actuator with it).
    pub(crate) fn forget(&mut self, device: DeviceKey) {
        self.playing.retain(|p| p.device != device);
    }
}

/// One of the browser's pads, as far as rumble needs it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BrowserPad {
    /// Its place in `navigator.getGamepads()`.
    pub(crate) index: u32,
    /// The browser's `id` text.
    pub(crate) id: String,
    /// Whether it has a `vibrationActuator` that plays effects.
    pub(crate) rumble: bool,
}

/// The browser's pad for the `nth` connected pad of the engine's backend
/// that has `id` as its name, when that backend (gilrs) doesn't say where
/// in the browser's list a pad is: pads of one `id` are numbered in the
/// order the browser lists them, which is the order gilrs finds them in.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn pick<'a>(pads: &'a [BrowserPad], id: &str, nth: usize) -> Option<&'a BrowserPad> {
    pads.iter().filter(|p| p.id == id).nth(nth)
}

/// The page side: `navigator.getGamepads()` on the calling thread. Where
/// there is no such call (a worker), there are no pads and nothing plays.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Default)]
pub(crate) struct Browser;

#[cfg(target_arch = "wasm32")]
impl Browser {
    /// Whether `pad` (a browser `Gamepad`) has an actuator that plays effects.
    pub(crate) fn has_actuator(pad: &wasm_bindgen::JsValue) -> bool {
        Self::actuator(pad).is_some()
    }

    fn get(object: &wasm_bindgen::JsValue, key: &str) -> Option<wasm_bindgen::JsValue> {
        js_sys::Reflect::get(object, &wasm_bindgen::JsValue::from_str(key))
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null())
    }

    fn actuator(pad: &wasm_bindgen::JsValue) -> Option<wasm_bindgen::JsValue> {
        let actuator = Self::get(pad, "vibrationActuator")?;
        Self::get(&actuator, "playEffect")?;
        Some(actuator)
    }

    /// The pads connected now, in the browser's order.
    pub(crate) fn pads() -> Vec<BrowserPad> {
        let mut out = Vec::new();
        Self::each_pad(|slot, pad| {
            let index = Self::get(pad, "index")
                .and_then(|i| i.as_f64())
                .map_or(slot, |i| i as u32);
            let id = Self::get(pad, "id").and_then(|id| id.as_string()).unwrap_or_default();
            out.push(BrowserPad {
                index,
                id,
                rumble: Self::has_actuator(pad),
            });
        });
        out
    }

    /// Calls `each(slot, pad)` for every connected pad of `navigator.getGamepads()`.
    pub(crate) fn each_pad(mut each: impl FnMut(u32, &wasm_bindgen::JsValue)) {
        use wasm_bindgen::JsCast;
        let Some(navigator) = Self::get(&js_sys::global(), "navigator") else {
            return;
        };
        let Some(call) = Self::get(&navigator, "getGamepads").and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        else {
            return;
        };
        let Ok(list) = call.call0(&navigator) else { return };
        let length = Self::get(&list, "length").and_then(|l| l.as_f64()).unwrap_or(0.0) as u32;
        for slot in 0..length.min(32) {
            let Ok(pad) = js_sys::Reflect::get_u32(&list, slot) else {
                continue;
            };
            if pad.is_null() || pad.is_undefined() || Self::get(&pad, "connected").is_some_and(|c| c.is_falsy()) {
                continue;
            }
            each(slot, &pad);
        }
    }

    /// The `vibrationActuator` of the pad at browser `index`, when it has one.
    fn actuator_at(index: u32) -> Option<wasm_bindgen::JsValue> {
        let mut found = None;
        Self::each_pad(|slot, pad| {
            let at = Self::get(pad, "index")
                .and_then(|i| i.as_f64())
                .map_or(slot, |i| i as u32);
            if at == index && found.is_none() {
                found = Self::actuator(pad);
            }
        });
        found
    }

    /// Runs `call` on the actuator's method, dropping the promise it
    /// returns so a refusal (a pad that went away) isn't an unhandled
    /// rejection in the console.
    fn call(actuator: &wasm_bindgen::JsValue, method: &str, args: &[wasm_bindgen::JsValue]) {
        use wasm_bindgen::JsCast;
        let Some(function) = Self::get(actuator, method).and_then(|f| f.dyn_into::<js_sys::Function>().ok()) else {
            return;
        };
        let result = match args {
            [] => function.call0(actuator),
            [a, b] => function.call2(actuator, a, b),
            _ => return,
        };
        if let Ok(promise) = result.and_then(|p| p.dyn_into::<js_sys::Promise>()) {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
            });
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Actuators for Browser {
    fn play(&mut self, index: u32, strong: f32, weak: f32, duration_ms: u32) {
        use wasm_bindgen::JsValue;
        let Some(actuator) = Self::actuator_at(index) else {
            return;
        };
        let params = js_sys::Object::new();
        for (key, value) in [
            ("startDelay", 0.0),
            ("duration", f64::from(duration_ms)),
            ("weakMagnitude", f64::from(weak.clamp(0.0, 1.0))),
            ("strongMagnitude", f64::from(strong.clamp(0.0, 1.0))),
        ] {
            let _ = js_sys::Reflect::set(&params, &JsValue::from_str(key), &JsValue::from_f64(value));
        }
        Self::call(
            &actuator,
            "playEffect",
            &[JsValue::from_str("dual-rumble"), params.into()],
        );
    }

    fn stop(&mut self, index: u32) {
        if let Some(actuator) = Self::actuator_at(index) {
            Self::call(&actuator, "reset", &[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        Play {
            index: u32,
            strong: f32,
            weak: f32,
            ms: u32,
        },
        Stop(u32),
    }

    /// Records what the browser would be asked, on a clock the test moves.
    struct Fake {
        calls: Vec<Call>,
        start: Instant,
        elapsed: Rc<Cell<Duration>>,
    }

    impl Fake {
        fn new() -> (Self, Rc<Cell<Duration>>) {
            let elapsed = Rc::new(Cell::new(Duration::ZERO));
            let fake = Fake {
                calls: Vec::new(),
                start: Instant::now(),
                elapsed: elapsed.clone(),
            };
            (fake, elapsed)
        }
        fn take(&mut self) -> Vec<Call> {
            std::mem::take(&mut self.calls)
        }
    }

    impl Actuators for Fake {
        fn now(&self) -> Instant {
            self.start + self.elapsed.get()
        }
        fn play(&mut self, index: u32, strong: f32, weak: f32, ms: u32) {
            self.calls.push(Call::Play {
                index,
                strong,
                weak,
                ms,
            });
        }
        fn stop(&mut self, index: u32) {
            self.calls.push(Call::Stop(index));
        }
    }

    fn levels(strong: f32, weak: f32) -> MotorLevels {
        MotorLevels {
            strong,
            weak,
            ..MotorLevels::OFF
        }
    }

    const PAD: DeviceKey = DeviceKey(2);

    fn play(strong: f32, weak: f32) -> Call {
        Call::Play {
            index: 2,
            strong,
            weak,
            ms: EFFECT_MS,
        }
    }

    #[test]
    fn an_effect_is_shorter_than_the_browsers_cap_and_is_replayed_before_it_ends() {
        assert!(EFFECT_MS < 5000, "browsers cap an effect at 5 s");
        assert!(
            RESEND < Duration::from_millis(u64::from(EFFECT_MS)),
            "replayed before it ends"
        );
    }

    #[test]
    fn rumble_plays_at_once_with_the_engines_levels() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(0.8, 0.3));
        assert_eq!(fake.take(), [play(0.8, 0.3)]);
    }

    #[test]
    fn rumble_is_sent_again_before_the_browsers_cap_for_as_long_as_it_plays() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(1.0, 0.5));
        fake.take();
        // Every frame for 12 seconds (a rumble that outlasts two caps).
        let mut sent = Vec::new();
        for frame in 1..=(12 * 60) {
            let at = Duration::from_millis(frame * 1000 / 60);
            clock.set(at);
            rumble.refresh(&mut fake);
            for call in fake.take() {
                assert_eq!(call, play(1.0, 0.5), "the same levels again");
                sent.push(at);
            }
        }
        assert!(sent.len() >= 11, "sent again about every second: {sent:?}");
        let mut last = Duration::ZERO;
        for at in sent {
            assert!(
                at - last < Duration::from_secs(5),
                "a gap reached the cap: {last:?} to {at:?}"
            );
            assert!(at - last >= RESEND, "not sooner than needed: {last:?} to {at:?}");
            last = at;
        }
    }

    #[test]
    fn nothing_is_sent_between_the_resends() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(1.0, 0.0));
        fake.take();
        clock.set(RESEND - Duration::from_millis(1));
        rumble.refresh(&mut fake);
        assert!(fake.take().is_empty());
    }

    #[test]
    fn off_stops_the_pad_once_and_ends_the_resending() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(1.0, 1.0));
        fake.take();
        rumble.set(&mut fake, PAD, 2, MotorLevels::OFF);
        assert_eq!(fake.take(), [Call::Stop(2)]);
        rumble.set(&mut fake, PAD, 2, MotorLevels::OFF);
        clock.set(Duration::from_secs(10));
        rumble.refresh(&mut fake);
        assert!(
            fake.take().is_empty(),
            "no stop for a pad that wasn't playing, no replay after off"
        );
    }

    #[test]
    fn trigger_motors_alone_play_nothing() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        let triggers = MotorLevels {
            left_trigger: 1.0,
            right_trigger: 1.0,
            ..MotorLevels::OFF
        };
        rumble.set(&mut fake, PAD, 2, triggers);
        assert!(fake.take().is_empty(), "the browser's body motors only");
    }

    #[test]
    fn new_levels_replace_the_old_ones_and_restart_the_resend_clock() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(1.0, 0.0));
        clock.set(Duration::from_millis(900));
        rumble.set(&mut fake, PAD, 2, levels(0.2, 0.4));
        fake.take();
        clock.set(Duration::from_millis(1500));
        rumble.refresh(&mut fake);
        assert!(fake.take().is_empty(), "the clock restarted at the change (0.9 s)");
        clock.set(Duration::from_millis(1900));
        rumble.refresh(&mut fake);
        assert_eq!(fake.take(), [play(0.2, 0.4)], "resent with the new levels");
    }

    #[test]
    fn a_pad_that_is_gone_is_forgotten_without_a_call() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(1.0, 1.0));
        rumble.set(&mut fake, DeviceKey(3), 3, levels(0.5, 0.5));
        fake.take();
        rumble.forget(PAD);
        clock.set(Duration::from_secs(2));
        rumble.refresh(&mut fake);
        assert_eq!(
            fake.take(),
            [Call::Play {
                index: 3,
                strong: 0.5,
                weak: 0.5,
                ms: EFFECT_MS
            }],
            "only the other pad plays on"
        );
    }

    #[test]
    fn a_pad_that_moved_in_the_browsers_list_stops_where_it_was() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, levels(1.0, 1.0));
        fake.take();
        rumble.set(&mut fake, PAD, 1, levels(1.0, 1.0));
        assert_eq!(
            fake.take(),
            [
                Call::Stop(2),
                Call::Play {
                    index: 1,
                    strong: 1.0,
                    weak: 1.0,
                    ms: EFFECT_MS
                }
            ]
        );
    }

    fn browser(index: u32, id: &str, rumble: bool) -> BrowserPad {
        BrowserPad {
            index,
            id: id.into(),
            rumble,
        }
    }

    #[test]
    fn identical_pads_are_told_apart_by_the_order_the_browser_lists_them() {
        let pads = [browser(0, "A", true), browser(1, "B", false), browser(3, "A", true)];
        assert_eq!(pick(&pads, "A", 0).map(|p| p.index), Some(0));
        assert_eq!(pick(&pads, "A", 1).map(|p| p.index), Some(3));
        assert_eq!(pick(&pads, "A", 2), None);
        assert_eq!(pick(&pads, "B", 0).map(|p| p.rumble), Some(false));
        assert_eq!(pick(&pads, "C", 0), None);
    }
}
