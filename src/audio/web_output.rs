#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub(super) use browser::{BrowserOutput, BrowserStatus, warn_unavailable};

fn output_channels(max_channels: u32) -> u16 {
    if max_channels == 1 { 1 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::output_channels;

    #[test]
    fn unknown_output_capacity_falls_back_to_stereo() {
        assert_eq!(output_channels(0), 2);
    }

    #[test]
    fn known_output_capacity_keeps_mono_or_stereo() {
        assert_eq!(output_channels(1), 1);
        assert_eq!(output_channels(2), 2);
        assert_eq!(output_channels(8), 2);
        assert_eq!(output_channels(u32::MAX), 2);
    }
}
