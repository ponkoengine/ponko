//! The output device: listing the devices, opening the chosen one, and
//! opening one again when it goes away, without restarting the game.
//!
//! Kira's stream already moves to the system's default device when its
//! device disconnects (and, on Windows and Linux, when the default
//! changes; on macOS the system reroutes the default device's stream
//! itself). The engine reports that ([`AudioDeviceEvent`]), opens the whole
//! output again when the device is gone (every voice carries on from where
//! it was, as when it comes back from being virtual), and goes back to the
//! device the player chose when it comes back.
//!
//! [`Outputs`] is what the engine asks the system: the real one through
//! cpal, an in-memory one for [`Audio::open_offline`](crate::audio::Audio::open_offline),
//! and in tests a fake whose devices come and go.

use super::super::dsp::buffer::Output;
use super::super::tracks::{BusTracks, Manager};
use super::Audio;
use crate::audio::{AudioDevice, AudioDeviceEvent};

/// How often the engine looks for a lost device to come back, in seconds.
const RETRY_SECONDS: f32 = 2.0;

/// What happened to the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// The browser reports nothing of its device.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(in crate::audio) enum DeviceIssue {
    /// The device went away: the output has to be opened again.
    Lost,
    /// The system moved the stream to its new default device.
    Rerouted,
}

/// An opened output: the mixer, the device it plays on, and for an
/// in-memory output the side the test reads.
pub(in crate::audio) struct Opened {
    pub manager: Manager,
    pub device: Option<AudioDevice>,
    pub buffer: Option<Output>,
}

/// What the engine asks the system about output devices.
pub(in crate::audio) trait Outputs {
    /// The devices sound can go to now.
    fn list(&mut self) -> Vec<AudioDevice>;
    /// Opens the device whose id or name is `device` (the default for
    /// `None`); `None` if it isn't there or won't open.
    fn open(&mut self, device: Option<&str>) -> Option<Opened>;
    /// The next thing that happened to `manager`'s stream.
    fn issue(&mut self, manager: &mut Manager) -> Option<DeviceIssue>;
}

/// Whether `device` is the one `wanted` names (by id or name).
fn names(device: &AudioDevice, wanted: &str) -> bool {
    device.id == wanted || device.name == wanted
}

/// The system's devices, or the browser's one output.
pub(in crate::audio) struct SystemOutputs;

#[cfg(not(target_arch = "wasm32"))]
impl Outputs for SystemOutputs {
    fn list(&mut self) -> Vec<AudioDevice> {
        use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
        let host = kira::backend::cpal::cpal::default_host();
        let default = host
            .default_output_device()
            .and_then(|d| d.id().ok())
            .map(|id| id.to_string());
        let Ok(devices) = host.output_devices() else {
            return Vec::new();
        };
        devices
            .filter_map(|d| {
                let id = d.id().ok()?.to_string();
                Some(AudioDevice {
                    default: default.as_deref() == Some(id.as_str()),
                    name: d.to_string(),
                    id,
                })
            })
            .collect()
    }
    fn open(&mut self, device: Option<&str>) -> Option<Opened> {
        use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
        use kira::backend::cpal::{CpalBackendSettings, cpal};
        let host = cpal::default_host();
        let chosen = match device {
            Some(wanted) => Some(
                host.output_devices()
                    .ok()?
                    .find(|d| d.id().is_ok_and(|id| id.to_string() == wanted) || d.to_string() == wanted)?,
            ),
            None => None,
        };
        let described = chosen.clone().or_else(|| host.default_output_device()).and_then(|d| {
            Some(AudioDevice {
                id: d.id().ok()?.to_string(),
                name: d.to_string(),
                default: device.is_none(),
            })
        });
        let settings = kira::AudioManagerSettings {
            backend_settings: CpalBackendSettings {
                device: chosen,
                config: None,
            },
            ..Default::default()
        };
        let manager = kira::AudioManager::new(settings).ok()?;
        Some(Opened {
            manager: Manager::Device(manager),
            device: described,
            buffer: None,
        })
    }
    fn issue(&mut self, manager: &mut Manager) -> Option<DeviceIssue> {
        let Manager::Device(m) = manager else {
            return None;
        };
        while let Some(error) = m.backend_mut().pop_error() {
            use kira::backend::cpal::cpal::ErrorKind;
            match error.kind() {
                ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated => return Some(DeviceIssue::Lost),
                ErrorKind::DeviceChanged => return Some(DeviceIssue::Rerouted),
                _ => {}
            }
        }
        None
    }
}

/// The web: one output, with device routing managed by the browser.
#[cfg(target_arch = "wasm32")]
impl Outputs for SystemOutputs {
    fn list(&mut self) -> Vec<AudioDevice> {
        vec![browser()]
    }
    fn open(&mut self, _device: Option<&str>) -> Option<Opened> {
        use super::super::web_output::{BrowserOutput, BrowserStatus, warn_unavailable};
        let status = BrowserStatus::default();
        let settings = kira::AudioManagerSettings {
            backend_settings: status.clone(),
            ..Default::default()
        };
        let manager = match kira::AudioManager::<BrowserOutput>::new(settings) {
            Ok(manager) => manager,
            Err(error) => {
                warn_unavailable(&error);
                return None;
            }
        };
        Some(Opened {
            manager: Manager::Browser(manager, status),
            device: Some(browser()),
            buffer: None,
        })
    }
    fn issue(&mut self, _manager: &mut Manager) -> Option<DeviceIssue> {
        None
    }
}
#[cfg(target_arch = "wasm32")]
fn browser() -> AudioDevice {
    AudioDevice {
        id: "default".into(),
        name: "Browser".into(),
        default: true,
    }
}

/// An in-memory output that nothing hears: the mix is rendered on demand.
pub(in crate::audio) struct OfflineOutputs {
    pub sample_rate: u32,
}
impl Outputs for OfflineOutputs {
    fn list(&mut self) -> Vec<AudioDevice> {
        vec![offline()]
    }
    fn open(&mut self, _device: Option<&str>) -> Option<Opened> {
        let (manager, buffer) = Manager::buffer(self.sample_rate)?;
        Some(Opened {
            manager,
            device: Some(offline()),
            buffer: Some(buffer),
        })
    }
    fn issue(&mut self, _manager: &mut Manager) -> Option<DeviceIssue> {
        None
    }
}
fn offline() -> AudioDevice {
    AudioDevice {
        id: "offline".into(),
        name: "Offline".into(),
        default: true,
    }
}

/// Which device was asked for, which one plays, and what happened.
pub(in crate::audio) struct OutputState {
    outputs: Box<dyn Outputs>,
    /// The device asked for by id or name; `None`: the system's default.
    pub chosen: Option<String>,
    /// The device playing now.
    pub current: Option<AudioDevice>,
    /// The chosen device is gone: the default plays while the engine
    /// looks for it.
    missing: bool,
    /// Every device is gone: the engine keeps trying to open one.
    retry: bool,
    since: f32,
    /// What happened this frame.
    pub events: Vec<AudioDeviceEvent>,
    /// The offline output's side to render from.
    pub offline: Option<Output>,
}
impl Default for OutputState {
    fn default() -> Self {
        Self::with(Box::new(SystemOutputs))
    }
}
impl OutputState {
    pub fn with(outputs: Box<dyn Outputs>) -> Self {
        Self {
            outputs,
            chosen: None,
            current: None,
            missing: false,
            retry: false,
            since: 0.0,
            events: Vec::new(),
            offline: None,
        }
    }
    /// Opens the chosen device, or the default if it isn't there.
    pub fn open_chosen(&mut self) -> Option<Manager> {
        let mut opened = None;
        if let Some(chosen) = self.chosen.clone() {
            opened = self.outputs.open(Some(&chosen));
            self.missing = opened.is_none();
        }
        let opened = opened.or_else(|| self.outputs.open(None));
        let Some(opened) = opened else {
            self.current = None;
            return None;
        };
        self.current = opened.device;
        self.offline = opened.buffer;
        Some(opened.manager)
    }
    pub fn list(&mut self) -> Vec<AudioDevice> {
        self.outputs.list()
    }
}

impl Audio {
    /// Opens the output again, on the chosen device or the default: every
    /// voice goes virtual first, so it carries on from where it was.
    /// Whether a device opened.
    pub(super) fn reopen(&mut self) -> bool {
        let music_at = self.music_position();
        let ids: Vec<u64> = self.playing.keys().copied().collect();
        for id in ids {
            let stopping = self.playing.get(&id).is_some_and(|v| v.stopping);
            if stopping {
                self.playing.remove(&id);
            } else {
                self.virtualize(id);
            }
        }
        // The old output goes first: a device can't always be open twice.
        self.tracks = BusTracks::default();
        self.reverb = None;
        self.drop_music_clock();
        self.manager = None;
        let opened = self.output.open_chosen();
        let ok = opened.is_some();
        self.manager = Some(opened);
        self.tracks.push(&self.buses);
        if ok && let Some(at) = music_at {
            self.restart_music_clock(at);
        }
        if ok {
            // Heard again at once, not a frame later.
            self.rank();
        }
        ok
    }

    /// The frame's look at the output: what happened to the stream, and
    /// whether a lost device came back.
    pub(super) fn poll_output(&mut self, dt: f32) {
        // Last frame's news is old now.
        self.output.events.clear();
        self.output.since += dt.max(0.0);
        let mut lost = false;
        let mut rerouted = false;
        if let Some(Some(manager)) = &mut self.manager {
            while let Some(issue) = self.output.outputs.issue(manager) {
                match issue {
                    DeviceIssue::Lost => lost = true,
                    DeviceIssue::Rerouted => rerouted = true,
                }
            }
        }
        if lost {
            let gone = self.output.current.as_ref().map(|d| d.name.clone()).unwrap_or_default();
            self.output.events.push(AudioDeviceEvent::Lost(gone));
            self.output.since = 0.0;
            if self.reopen() {
                self.switched();
            } else {
                self.output.retry = true;
            }
            return;
        }
        if rerouted {
            // The system's default moved: say where sound plays now.
            let now = self.output.list().into_iter().find(|d| d.default);
            if let Some(now) = now {
                self.output.current = Some(now);
                self.switched();
            }
        }
        if (self.output.missing || self.output.retry) && self.output.since >= RETRY_SECONDS {
            self.output.since = 0.0;
            let back = match self.output.chosen.clone() {
                Some(chosen) if self.output.missing => self.output.list().iter().any(|d| names(d, &chosen)),
                _ => true,
            };
            if back && self.reopen() {
                self.output.retry = false;
                self.switched();
            }
        }
    }

    fn switched(&mut self) {
        let name = self.output.current.as_ref().map(|d| d.name.clone()).unwrap_or_default();
        self.output.events.push(AudioDeviceEvent::Switched(name));
    }

    pub fn output_devices(&mut self) -> Vec<AudioDevice> {
        self.output.list()
    }

    /// Plays on `device` (an id or name from `output_devices`; `None`: the
    /// system's default) from now on, opening the output again if it is open.
    pub fn set_output_device(&mut self, device: Option<&str>) -> Result<(), String> {
        if let Some(wanted) = device
            && !self.output.list().iter().any(|d| names(d, wanted))
        {
            return Err(format!("no audio output device \"{wanted}\""));
        }
        self.output.chosen = device.map(str::to_owned);
        self.output.missing = false;
        if self.manager.is_some() {
            if self.reopen() {
                self.output.retry = false;
                self.switched();
            } else {
                self.output.retry = true;
            }
        }
        Ok(())
    }

    /// Mixes into memory from now on instead of a device.
    pub fn open_offline(&mut self, sample_rate: u32) {
        self.output = OutputState::with(Box::new(OfflineOutputs {
            sample_rate: sample_rate.max(1),
        }));
        self.reopen();
    }

    pub fn output_device(&self) -> Option<&AudioDevice> {
        self.output.current.as_ref()
    }
    pub fn device_events(&self) -> &[AudioDeviceEvent] {
        &self.output.events
    }

    /// The next `seconds` of the offline mix.
    pub fn render_offline(&mut self, seconds: f32, _sample_rate: u32) -> Vec<[f32; 2]> {
        self.output
            .offline
            .as_ref()
            .map_or_else(Vec::new, |o| o.render(seconds))
    }

    /// For tests: the backend on a set of fake devices.
    #[cfg(test)]
    pub(in crate::audio) fn with_outputs(outputs: Box<dyn Outputs>) -> Self {
        Self {
            output: OutputState::with(outputs),
            ..Default::default()
        }
    }
}

/// Devices that come and go as a test says, each playing into a buffer.
#[cfg(test)]
pub(in crate::audio) mod fake {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub(in crate::audio) struct Devices {
        pub list: Vec<AudioDevice>,
        /// Things to happen to the stream, oldest first.
        pub issues: std::collections::VecDeque<DeviceIssue>,
        /// The device each output was opened on, oldest first, and its buffer.
        pub opened: Vec<(String, Output)>,
    }
    #[derive(Clone, Default)]
    pub(in crate::audio) struct FakeOutputs(pub Arc<Mutex<Devices>>);
    impl FakeOutputs {
        pub fn device(id: &str, default: bool) -> AudioDevice {
            AudioDevice {
                id: id.into(),
                name: format!("{id} speakers"),
                default,
            }
        }
    }
    impl Outputs for FakeOutputs {
        fn list(&mut self) -> Vec<AudioDevice> {
            self.0.lock().unwrap().list.clone()
        }
        fn open(&mut self, device: Option<&str>) -> Option<Opened> {
            let mut devices = self.0.lock().unwrap();
            let found = match device {
                Some(wanted) => devices.list.iter().find(|d| names(d, wanted))?.clone(),
                None => devices.list.iter().find(|d| d.default)?.clone(),
            };
            let (manager, buffer) = Manager::buffer(48_000)?;
            let reader = super::super::super::dsp::buffer::Output {
                renderer: buffer.renderer.clone(),
                sample_rate: buffer.sample_rate,
            };
            devices.opened.push((found.id.clone(), reader));
            Some(Opened {
                manager,
                device: Some(found),
                buffer: Some(buffer),
            })
        }
        fn issue(&mut self, _manager: &mut Manager) -> Option<DeviceIssue> {
            self.0.lock().unwrap().issues.pop_front()
        }
    }
}
