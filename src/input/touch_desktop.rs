//! Whether a desktop has a touch screen, so on-screen controls set to
//! [`TouchShow::Auto`](super::touch::TouchShow::Auto) show before the first
//! finger. The touches themselves come from the window on every desktop
//! that has them (Windows, Linux on X11 and Wayland); a Mac has no touch
//! screen and says no.

/// Whether this desktop has a touch screen.
#[cfg(feature = "window")]
pub(crate) fn has_touch_screen() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_touch_points() > 0
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        std::fs::read_to_string("/proc/bus/input/devices").is_ok_and(|devices| linux_touch_screen(&devices))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "freebsd")))]
    {
        false
    }
}

/// How many touches the system's touch screens take at once (0 without
/// one): Windows' `SM_MAXIMUMTOUCHES`.
#[cfg(all(target_os = "windows", feature = "window"))]
fn windows_touch_points() -> i32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetSystemMetrics(index: i32) -> i32;
    }
    const SM_MAXIMUMTOUCHES: i32 = 95;
    // SAFETY: `GetSystemMetrics` takes an index by value and returns an
    // integer; it reads no memory of ours and has no preconditions.
    unsafe { GetSystemMetrics(SM_MAXIMUMTOUCHES) }
}

/// Whether the kernel's list of input devices (`/proc/bus/input/devices`)
/// has a touch screen: a device that points directly at the screen
/// (`INPUT_PROP_DIRECT`) and reports several fingers' positions
/// (`ABS_MT_POSITION_X`). A trackpad points indirectly; a pen tablet
/// reports one position.
#[cfg_attr(not(all(feature = "window", any(target_os = "linux", target_os = "freebsd"))), allow(dead_code))]
fn linux_touch_screen(devices: &str) -> bool {
    const INPUT_PROP_DIRECT: u32 = 1;
    const ABS_MT_POSITION_X: u32 = 0x35;
    devices.split("\n\n").any(|device| {
        let bitmap = |key: &str| {
            device
                .lines()
                .find_map(|line| line.strip_prefix("B: ")?.strip_prefix(key)?.strip_prefix('='))
        };
        let has = |key: &str, bit: u32| bitmap(key).is_some_and(|words| bit_set(words, bit));
        has("PROP", INPUT_PROP_DIRECT) && has("ABS", ABS_MT_POSITION_X)
    })
}

/// Whether bit `bit` is set in a kernel bitmap: hexadecimal words separated
/// by spaces, the highest first, each as wide as the kernel's `long`
/// (64 bits on the machines that run games; a word wider than 32 bits'
/// digits says which).
#[cfg_attr(not(all(feature = "window", any(target_os = "linux", target_os = "freebsd"))), allow(dead_code))]
fn bit_set(words: &str, bit: u32) -> bool {
    let words: Vec<&str> = words.split_whitespace().collect();
    let width = if words.iter().any(|w| w.len() > 8) || cfg!(target_pointer_width = "64") {
        64
    } else {
        32
    };
    let (index, shift) = ((bit / width) as usize, bit % width);
    words
        .len()
        .checked_sub(index + 1)
        .and_then(|i| u64::from_str_radix(words[i], 16).ok())
        .is_some_and(|word| word >> shift & 1 == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A laptop's touch screen, its trackpad and a pen tablet, as Linux
    /// lists them (cut down to what matters).
    const DEVICES: &str = "I: Bus=0018 Vendor=04f3 Product=2a1c Version=0100
N: Name=\"ELAN Touchscreen\"
B: PROP=2
B: EV=b
B: KEY=400 0 0 0 0 0
B: ABS=3273800000000003

I: Bus=0018 Vendor=06cb Product=cd8b Version=0100
N: Name=\"SYNA Touchpad\"
B: PROP=5
B: EV=b
B: ABS=2e0800000000003

I: Bus=0003 Vendor=056a Product=0357 Version=0110
N: Name=\"Pen Tablet\"
B: PROP=2
B: EV=1b
B: ABS=1000f000003
";

    #[test]
    fn a_direct_multitouch_device_is_a_touch_screen() {
        assert!(linux_touch_screen(DEVICES));
    }

    #[test]
    fn a_trackpad_and_a_pen_tablet_are_not_touch_screens() {
        let without = DEVICES.split("\n\n").skip(1).collect::<Vec<_>>().join("\n\n");
        assert!(!linux_touch_screen(&without));
        assert!(!linux_touch_screen(""));
    }

    #[test]
    fn kernel_bitmaps_are_read_highest_word_first() {
        assert!(bit_set("1 0", 64));
        assert!(!bit_set("1 0", 0));
        assert!(bit_set("3273800000000003", 0x35));
        assert!(!bit_set("zz", 0));
    }

    #[cfg(all(target_os = "windows", feature = "window"))]
    #[test]
    fn windows_says_how_many_touches_its_screens_take() {
        // A machine without a touch screen says 0; never negative.
        assert!(windows_touch_points() >= 0);
        let _ = has_touch_screen();
    }
}
