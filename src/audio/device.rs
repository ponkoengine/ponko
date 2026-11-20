//! The output device, and mixing offline: which devices there are
//! ([`Audio::output_devices`]), playing on one of them
//! ([`Audio::set_output_device`]), what happened to it this frame
//! ([`Audio::device_events`]), and an output that renders into memory for
//! tests and tools ([`Audio::open_offline`]).
//!
//! When the device playing goes away (headphones unplugged), the engine
//! opens the output again on the system's default device and every sound
//! carries on from where it was; when a device the player chose comes
//! back, it goes back to it. No restart, and nothing for the game to do
//! but tell the player, if it wants to.

use super::Audio;
use crate::error::{Error, ErrorKind};

/// An output device sound can play on.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     ctx.audio.open_offline(48_000);
///     let devices = ctx.audio.output_devices();
///     assert!(devices[0].default);
/// });
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct AudioDevice {
    /// What the system calls it, stable across runs where the system can
    /// manage it: save this in the settings.
    pub id: String,
    /// Its name, to show the player.
    pub name: String,
    /// Whether it is the system's default output.
    pub default: bool,
}

/// Something that happened to the output this frame. As JSON (the C
/// API's form): `{"lost": "name"}` or `{"switched": "name"}`.
///
/// ```
/// use ponko::audio::AudioDeviceEvent;
/// let e = AudioDeviceEvent::Switched("Speakers".into());
/// assert!(matches!(e, AudioDeviceEvent::Switched(name) if name == "Speakers"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AudioDeviceEvent {
    /// The device playing (named) went away; a `Switched` follows once
    /// sound plays again.
    Lost(String),
    /// Sound plays on this device (named) now: the system's default after
    /// a loss, the chosen device when it came back, or a new choice.
    Switched(String),
}

impl Audio {
    /// The output devices sound can play on now. Empty in a headless run
    /// (which never touches the system's devices) and without the `audio`
    /// feature; on the web, the browser's one output.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| assert!(ctx.audio.output_devices().is_empty()));
    /// ```
    pub fn output_devices(&mut self) -> Vec<crate::audio::AudioDevice> {
        if self.headless && !self.offline {
            return Vec::new();
        }
        self.backend.output_devices()
    }

    /// Plays on `device`, an [`AudioDevice::id`] (or name) from
    /// [`output_devices`](Self::output_devices), from now on; `None` follows
    /// the system's default. If the output is open, it opens again on the
    /// new device and every sound carries on. The choice lasts while the
    /// device is unplugged: sound plays on the default meanwhile and goes
    /// back when it returns. A headless run only remembers the choice.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] if no device has that id or name; nothing changes.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.open_offline(48_000);
    ///     assert!(ctx.audio.set_output_device(Some("no such device")).is_err());
    ///     ctx.audio.set_output_device(Some("offline")).unwrap();
    ///     assert_eq!(ctx.audio.chosen_output_device(), Some("offline"));
    /// });
    /// ```
    pub fn set_output_device(&mut self, device: Option<&str>) -> Result<(), Error> {
        if self.headless && !self.offline {
            self.chosen_device = device.map(str::to_owned);
            return Ok(());
        }
        self.backend
            .set_output_device(device)
            .map_err(|message| Error::new(ErrorKind::NotFound, message))?;
        self.chosen_device = device.map(str::to_owned);
        Ok(())
    }

    /// The device asked for with [`set_output_device`](Self::set_output_device);
    /// `None` follows the system's default.
    #[must_use]
    pub fn chosen_output_device(&self) -> Option<&str> {
        self.chosen_device.as_deref()
    }

    /// The device sound plays on now; `None` before the output opens (at
    /// the first sound), in a headless run and without a device.
    #[must_use]
    pub fn output_device(&self) -> Option<&crate::audio::AudioDevice> {
        self.backend.output_device()
    }

    /// What happened to the output this frame, oldest first: emptied when
    /// the next frame begins.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::audio::AudioDeviceEvent;
    /// # fn f(ctx: &mut Ctx) {
    /// for event in ctx.audio.device_events() {
    ///     if let AudioDeviceEvent::Switched(name) = event {
    ///         println!("sound now plays on {name}");
    ///     }
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn device_events(&self) -> &[crate::audio::AudioDeviceEvent] {
        self.backend.device_events()
    }

    /// Mixes into memory instead of a device, from now on, at
    /// `sample_rate` frames per second: nothing is heard, and
    /// [`render_offline`](Self::render_offline) pulls the mix out. Works
    /// in headless runs, which play nothing otherwise, so a test can check
    /// what a game's mix sounds like: levels, ducking, music changes.
    /// Sounds already playing carry on in the new output. Without the
    /// `audio` feature the mix is silence.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.audio.open_offline(48_000);
    ///     let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    ///     let tone = ctx.audio.create(48_000, &tone);
    ///     ctx.audio.play(tone);
    ///     let mix = ctx.audio.render_offline(0.1);
    ///     assert_eq!(mix.len(), 4800);
    /// #   #[cfg(feature = "audio")]
    ///     assert!(mix.iter().any(|f| f[0].abs() > 0.1));
    /// });
    /// ```
    pub fn open_offline(&mut self, sample_rate: u32) {
        self.offline = true;
        self.offline_rate = sample_rate.max(1);
        self.backend.open_offline(sample_rate);
    }

    /// The next `seconds` of the offline mix, as (left, right) frames;
    /// empty unless [`open_offline`](Self::open_offline) was called. The
    /// engine moves fades, music and voice limits once a frame; between
    /// frames, render the frame's time.
    #[must_use]
    pub fn render_offline(&mut self, seconds: f32) -> Vec<[f32; 2]> {
        if !self.offline {
            return Vec::new();
        }
        self.backend.render_offline(seconds, self.offline_rate)
    }
}
