//! Glue to one platform's own system APIs, behind the engine's
//! platform-neutral ones ([`WindowEvent::Suspended`](crate::window::WindowEvent::Suspended),
//! [`WindowService::safe_area`](crate::window::WindowService::safe_area),
//! [`WindowService::keyboard_inset`](crate::window::WindowService::keyboard_inset),
//! [`AudioSessionCategory`](crate::audio::AudioSessionCategory)). What
//! every platform shares (the math from a platform's frames to insets, the
//! lifecycle state) lives with those APIs and is tested on any machine.

#[cfg(target_os = "ios")]
pub(crate) mod ios;

#[cfg(all(target_os = "android", feature = "window"))]
pub(crate) mod android;
