//! The audio session's category ([`Audio::set_session_category`]): how the
//! game's sound shares the device with other apps' audio and with a
//! phone's ring/silent switch. On iOS it is the `AVAudioSession` category;
//! elsewhere it is kept and changes nothing (desktops and the web have no
//! such switch, and their sound always mixes with other apps').
//!
//! Interruptions (a call, an alarm, Siri) need nothing from the game: the
//! system stops the output, and the engine's audio backend starts it again
//! when the interruption ends and the system says it may.

use super::Audio;
use std::sync::atomic::{AtomicU8, Ordering};

/// How the game's sound shares the device with other apps and with the
/// ring/silent switch ([`Audio::set_session_category`]). The names are
/// iOS's categories.
///
/// ```
/// use ponko::audio::AudioSessionCategory;
/// assert_eq!(AudioSessionCategory::default(), AudioSessionCategory::SoloAmbient);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AudioSessionCategory {
    /// The default: starting the game's sound stops other apps' audio (the
    /// player's music), and the silent switch mutes the game.
    #[default]
    SoloAmbient,
    /// The game's sound mixes with other apps' audio, so the player's own
    /// music keeps playing; the silent switch mutes the game. For games
    /// whose sound is a nicety.
    Ambient,
    /// The game is heard even with the silent switch on, and stops other
    /// apps' audio: for games whose sound is the point (music and rhythm
    /// games). Not playing in the background: that needs the app's
    /// `audio` background mode, which the engine doesn't ask for.
    Playback,
}

impl AudioSessionCategory {
    fn code(self) -> u8 {
        match self {
            Self::SoloAmbient => 0,
            Self::Ambient => 1,
            Self::Playback => 2,
        }
    }
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Ambient,
            2 => Self::Playback,
            _ => Self::SoloAmbient,
        }
    }
}

/// The audio session is one per process (as iOS's is).
static CATEGORY: AtomicU8 = AtomicU8::new(0);

impl Audio {
    /// Sets how the game's sound shares the device with other apps and the
    /// ring/silent switch, from now on (call it in setup to start with it;
    /// a settings screen may change it while playing). On iOS it sets the
    /// audio session's category; elsewhere it is kept, and does nothing.
    /// Fails only where the system refuses the category; the old one stays.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::audio::AudioSessionCategory;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     // Let the player's music play on under the game's sounds.
    ///     ctx.audio.set_session_category(AudioSessionCategory::Ambient).unwrap();
    ///     assert_eq!(ctx.audio.session_category(), AudioSessionCategory::Ambient);
    /// });
    /// ```
    pub fn set_session_category(&mut self, category: AudioSessionCategory) -> Result<(), crate::error::Error> {
        #[cfg(all(target_os = "ios", feature = "audio"))]
        crate::platform::ios::set_audio_category(category).map_err(|e| {
            crate::error::Error::new(
                crate::error::ErrorKind::Unsupported,
                format!("could not set the audio session's category to {category:?}: {e}"),
            )
        })?;
        CATEGORY.store(category.code(), Ordering::Relaxed);
        Ok(())
    }

    /// The category [`set_session_category`](Self::set_session_category)
    /// set: [`AudioSessionCategory::SoloAmbient`] until then.
    pub fn session_category(&self) -> AudioSessionCategory {
        AudioSessionCategory::from_code(CATEGORY.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_survives_its_code() {
        for c in [
            AudioSessionCategory::SoloAmbient,
            AudioSessionCategory::Ambient,
            AudioSessionCategory::Playback,
        ] {
            assert_eq!(AudioSessionCategory::from_code(c.code()), c);
        }
        assert_eq!(AudioSessionCategory::from_code(200), AudioSessionCategory::SoloAmbient);
    }
}
