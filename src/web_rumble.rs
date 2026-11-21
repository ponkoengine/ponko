//! Rumble in the browser, for the web's gamepad backend (`gamepad_web`).
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
//! Trigger motors follow the same rules as on the desktop: a pad whose
//! actuator lists the `trigger-rumble` effect (in `effects`, or through the
//! older `canPlayEffectType`) reports `capabilities.trigger_rumble` and
//! plays the trigger levels with the body motors' in one `trigger-rumble`
//! effect; on any other pad the trigger levels are dropped, the body motors
//! still play (`dual-rumble`), and a request for the triggers alone plays
//! nothing ([`WebEffect::for_pad`]).

use web_time::{Duration, Instant};

use crate::input::{DeviceKey, MotorLevels};

/// How long each effect asked of the browser lasts: well under its 5 s cap.
pub(crate) const EFFECT_MS: u32 = 2000;
/// How long a playing effect is left before the same levels are played
/// again: half of [`EFFECT_MS`], so a late frame never leaves a gap.
pub(crate) const RESEND: Duration = Duration::from_millis(1000);

/// One effect for a pad's `vibrationActuator`, levels `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WebEffect {
    /// `dual-rumble`: the two body motors.
    Dual { strong: f32, weak: f32 },
    /// `trigger-rumble`: the body motors and the two trigger motors.
    Trigger {
        strong: f32,
        weak: f32,
        left: f32,
        right: f32,
    },
}

impl WebEffect {
    /// The effect that plays `levels` on a pad with trigger motors
    /// (`triggers`) or without, by the desktop's rules: the triggers'
    /// levels reach a pad that has trigger motors and are dropped on one
    /// that hasn't, where the body motors still play. `None`: nothing to
    /// play (every motor the pad has is off).
    pub(crate) fn for_pad(levels: MotorLevels, triggers: bool) -> Option<WebEffect> {
        let MotorLevels {
            strong,
            weak,
            left_trigger: left,
            right_trigger: right,
        } = levels;
        if triggers && (left > 0.0 || right > 0.0) {
            Some(WebEffect::Trigger {
                strong,
                weak,
                left,
                right,
            })
        } else if strong > 0.0 || weak > 0.0 {
            Some(WebEffect::Dual { strong, weak })
        } else {
            None
        }
    }

    /// The browser's name for it, `playEffect`'s first argument.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            WebEffect::Dual { .. } => "dual-rumble",
            WebEffect::Trigger { .. } => "trigger-rumble",
        }
    }
}

/// What the browser is asked to do with a pad's `vibrationActuator`.
pub(crate) trait Actuators {
    /// The clock the re-sending counts on (a test's replaces it).
    fn now(&self) -> Instant {
        Instant::now()
    }
    /// `playEffect(effect.kind(), ...)` on the pad at browser `index`; a
    /// new effect replaces the one playing.
    fn play(&mut self, index: u32, effect: WebEffect, duration_ms: u32);
    /// `reset()`: stops whatever plays on that pad.
    fn stop(&mut self, index: u32);
}

/// A pad that is rumbling.
#[derive(Debug, Clone, Copy)]
struct Playing {
    device: DeviceKey,
    /// Its place in `navigator.getGamepads()`.
    index: u32,
    effect: WebEffect,
    sent: Instant,
}

/// The rumble playing on each pad, kept alive past the browser's cap.
#[derive(Debug, Default)]
pub(crate) struct WebRumble {
    playing: Vec<Playing>,
}

impl WebRumble {
    /// The engine's wish for `device` (browser pad `index`, with trigger
    /// motors or not): plays the levels the pad has motors for now
    /// ([`WebEffect::for_pad`]), or stops the pad when they are off.
    pub(crate) fn set(
        &mut self,
        actuators: &mut impl Actuators,
        device: DeviceKey,
        index: u32,
        triggers: bool,
        levels: MotorLevels,
    ) {
        let was = self.playing.iter().position(|p| p.device == device);
        let Some(effect) = WebEffect::for_pad(levels, triggers) else {
            if let Some(at) = was {
                let old = self.playing.swap_remove(at);
                actuators.stop(old.index);
            }
            return;
        };
        let now = actuators.now();
        let playing = Playing {
            device,
            index,
            effect,
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
        actuators.play(index, effect, EFFECT_MS);
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
                actuators.play(p.index, p.effect, EFFECT_MS);
            }
        }
    }

    /// The pad is gone: forgets it, with no call (the browser has dropped
    /// its actuator with it).
    pub(crate) fn forget(&mut self, device: DeviceKey) {
        self.playing.retain(|p| p.device != device);
    }
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

    /// Whether `pad`'s actuator plays `trigger-rumble`: it lists it in
    /// `effects` or, in browsers from before that list, says yes to
    /// `canPlayEffectType("trigger-rumble")`.
    pub(crate) fn has_trigger_actuator(pad: &wasm_bindgen::JsValue) -> bool {
        use wasm_bindgen::{JsCast, JsValue};
        let Some(actuator) = Self::actuator(pad) else {
            return false;
        };
        let wanted = JsValue::from_str("trigger-rumble");
        if let Some(effects) = Self::get(&actuator, "effects") {
            return js_sys::Array::is_array(&effects) && js_sys::Array::from(&effects).includes(&wanted, 0);
        }
        Self::get(&actuator, "canPlayEffectType")
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call1(&actuator, &wanted).ok())
            .is_some_and(|yes| yes.is_truthy())
    }

    fn actuator(pad: &wasm_bindgen::JsValue) -> Option<wasm_bindgen::JsValue> {
        let actuator = Self::get(pad, "vibrationActuator")?;
        Self::get(&actuator, "playEffect")?;
        Some(actuator)
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
    fn play(&mut self, index: u32, effect: WebEffect, duration_ms: u32) {
        use wasm_bindgen::JsValue;
        let Some(actuator) = Self::actuator_at(index) else {
            return;
        };
        let params = js_sys::Object::new();
        let set = |key: &str, value: f64| {
            let _ = js_sys::Reflect::set(&params, &JsValue::from_str(key), &JsValue::from_f64(value));
        };
        let level = |v: f32| f64::from(v.clamp(0.0, 1.0));
        set("startDelay", 0.0);
        set("duration", f64::from(duration_ms));
        match effect {
            WebEffect::Dual { strong, weak } => {
                set("weakMagnitude", level(weak));
                set("strongMagnitude", level(strong));
            }
            WebEffect::Trigger {
                strong,
                weak,
                left,
                right,
            } => {
                set("weakMagnitude", level(weak));
                set("strongMagnitude", level(strong));
                set("leftTrigger", level(left));
                set("rightTrigger", level(right));
            }
        }
        Self::call(
            &actuator,
            "playEffect",
            &[JsValue::from_str(effect.kind()), params.into()],
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
        Play { index: u32, effect: WebEffect, ms: u32 },
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
        fn play(&mut self, index: u32, effect: WebEffect, ms: u32) {
            self.calls.push(Call::Play { index, effect, ms });
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
        play_at(2, strong, weak)
    }

    fn play_at(index: u32, strong: f32, weak: f32) -> Call {
        Call::Play {
            index,
            effect: WebEffect::Dual { strong, weak },
            ms: EFFECT_MS,
        }
    }

    #[test]
    fn an_effect_is_shorter_than_the_browsers_cap_and_is_replayed_before_it_ends() {
        const { assert!(EFFECT_MS < 5000, "browsers cap an effect at 5 s") };
        assert!(
            RESEND < Duration::from_millis(u64::from(EFFECT_MS)),
            "replayed before it ends"
        );
    }

    #[test]
    fn rumble_plays_at_once_with_the_engines_levels() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, false, levels(0.8, 0.3));
        assert_eq!(fake.take(), [play(0.8, 0.3)]);
    }

    #[test]
    fn rumble_is_sent_again_before_the_browsers_cap_for_as_long_as_it_plays() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, false, levels(1.0, 0.5));
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
        rumble.set(&mut fake, PAD, 2, false, levels(1.0, 0.0));
        fake.take();
        clock.set(RESEND - Duration::from_millis(1));
        rumble.refresh(&mut fake);
        assert!(fake.take().is_empty());
    }

    #[test]
    fn off_stops_the_pad_once_and_ends_the_resending() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, false, levels(1.0, 1.0));
        fake.take();
        rumble.set(&mut fake, PAD, 2, false, MotorLevels::OFF);
        assert_eq!(fake.take(), [Call::Stop(2)]);
        rumble.set(&mut fake, PAD, 2, false, MotorLevels::OFF);
        clock.set(Duration::from_secs(10));
        rumble.refresh(&mut fake);
        assert!(
            fake.take().is_empty(),
            "no stop for a pad that wasn't playing, no replay after off"
        );
    }

    #[test]
    fn trigger_motors_alone_play_nothing_on_a_pad_without_them() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        let triggers = MotorLevels {
            left_trigger: 1.0,
            right_trigger: 1.0,
            ..MotorLevels::OFF
        };
        rumble.set(&mut fake, PAD, 2, false, triggers);
        assert!(fake.take().is_empty(), "the browser's body motors only");
    }

    #[test]
    fn new_levels_replace_the_old_ones_and_restart_the_resend_clock() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, false, levels(1.0, 0.0));
        clock.set(Duration::from_millis(900));
        rumble.set(&mut fake, PAD, 2, false, levels(0.2, 0.4));
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
        rumble.set(&mut fake, PAD, 2, false, levels(1.0, 1.0));
        rumble.set(&mut fake, DeviceKey(3), 3, false, levels(0.5, 0.5));
        fake.take();
        rumble.forget(PAD);
        clock.set(Duration::from_secs(2));
        rumble.refresh(&mut fake);
        assert_eq!(fake.take(), [play_at(3, 0.5, 0.5)], "only the other pad plays on");
    }

    #[test]
    fn a_pad_that_moved_in_the_browsers_list_stops_where_it_was() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, false, levels(1.0, 1.0));
        fake.take();
        rumble.set(&mut fake, PAD, 1, false, levels(1.0, 1.0));
        assert_eq!(fake.take(), [Call::Stop(2), play_at(1, 1.0, 1.0)]);
    }

    fn with_triggers(strong: f32, weak: f32, left: f32, right: f32) -> MotorLevels {
        MotorLevels {
            strong,
            weak,
            left_trigger: left,
            right_trigger: right,
        }
    }

    #[test]
    fn a_pad_with_trigger_motors_plays_all_four_motors_as_one_trigger_rumble() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, true, with_triggers(0.0, 0.2, 0.4, 0.9));
        let effect = WebEffect::Trigger {
            strong: 0.0,
            weak: 0.2,
            left: 0.4,
            right: 0.9,
        };
        assert_eq!(effect.kind(), "trigger-rumble");
        assert_eq!(
            fake.take(),
            [Call::Play {
                index: 2,
                effect,
                ms: EFFECT_MS
            }]
        );
    }

    #[test]
    fn trigger_levels_are_dropped_on_a_pad_without_trigger_motors_and_the_body_motors_still_play() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, false, with_triggers(0.0, 0.2, 0.4, 0.9));
        assert_eq!(fake.take(), [play(0.0, 0.2)], "dual-rumble, as the desktop drops them");
    }

    #[test]
    fn body_rumble_alone_stays_dual_rumble_on_a_pad_with_trigger_motors() {
        let (mut fake, _) = Fake::new();
        let mut rumble = WebRumble::default();
        rumble.set(&mut fake, PAD, 2, true, levels(0.8, 0.3));
        assert_eq!(fake.take(), [play(0.8, 0.3)]);
    }

    #[test]
    fn trigger_rumble_alone_plays_and_is_replayed_and_stopped_on_a_pad_with_trigger_motors() {
        let (mut fake, clock) = Fake::new();
        let mut rumble = WebRumble::default();
        let triggers = with_triggers(0.0, 0.0, 1.0, 0.5);
        rumble.set(&mut fake, PAD, 2, true, triggers);
        let effect = WebEffect::Trigger {
            strong: 0.0,
            weak: 0.0,
            left: 1.0,
            right: 0.5,
        };
        let want = Call::Play {
            index: 2,
            effect,
            ms: EFFECT_MS,
        };
        assert_eq!(fake.take(), std::slice::from_ref(&want));
        clock.set(RESEND);
        rumble.refresh(&mut fake);
        assert_eq!(fake.take(), [want], "replayed before the cap, as body rumble is");
        rumble.set(&mut fake, PAD, 2, true, MotorLevels::OFF);
        assert_eq!(fake.take(), [Call::Stop(2)]);
    }

    #[test]
    fn the_effect_for_each_pad_follows_the_desktop_rules() {
        let only_triggers = with_triggers(0.0, 0.0, 0.3, 0.0);
        assert_eq!(WebEffect::for_pad(only_triggers, false), None);
        assert!(matches!(
            WebEffect::for_pad(only_triggers, true),
            Some(WebEffect::Trigger { left, .. }) if left == 0.3
        ));
        assert_eq!(WebEffect::for_pad(MotorLevels::OFF, true), None);
        assert_eq!(
            WebEffect::for_pad(levels(0.5, 0.0), true).map(WebEffect::kind),
            Some("dual-rumble")
        );
    }
}
