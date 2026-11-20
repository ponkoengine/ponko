//! A video's soundtrack (feature `video`): the audio track of the video's
//! own encoded bytes, decoded a packet at a time as it plays, and one voice
//! that starts part-way into it and, like music, is never made virtual or
//! counted against the voice limits.

use super::{Audio, AudioBus, AudioEffects, Clip, SoundParams, StartTime, Voice};
use std::sync::Arc;

impl Audio {
    /// Keeps `bytes` (a container with an audio track) to play as a
    /// soundtrack `seconds` long. Its header is read now to fail early.
    pub fn soundtrack(&mut self, bytes: Arc<[u8]>, seconds: f32) -> Result<u32, String> {
        let header =
            crate::audio::chunked::header(&bytes).map_err(|e| format!("could not decode the soundtrack: {e}"))?;
        // A Matroska file doesn't give its audio track's length: the
        // container's is the video's, given by the caller.
        Ok(self.push(Clip::Chunked(bytes), seconds.max(header.seconds())))
    }

    /// Plays soundtrack `sound` on `bus` from `from` seconds in.
    pub fn play_soundtrack(&mut self, sound: u32, bus: AudioBus, from: f64) -> u64 {
        self.next += 1;
        let id = self.next;
        let mut voice = Voice::new(sound, SoundParams::default(), bus, AudioEffects::default());
        voice.exempt = true;
        voice.position = from.max(0.0);
        self.playing.insert(id, voice);
        self.realize(id, StartTime::Immediate, None);
        id
    }
}
