#!/usr/bin/env python3
"""How busy the machine is, as one number on every desktop OS.

    python3 scripts/cpu_load.py        # prints it, e.g. 0.42

macOS and Linux: the 1-minute load average (`os.getloadavg`). Windows has no
load average, so it is the number of busy logical processors averaged over
a few seconds (`GetSystemTimes`): 1.0 means one core's worth of work. Both
read the same way against a threshold: under 3, the machine is quiet enough
to measure on. The scoreboard (`bench/compare/scoreboard.py`) and
`scripts/build-times.sh` write it next to every run.
"""

import os
import sys
import time

# Windows: the window the busy-processor average is taken over.
SAMPLE_SECONDS = 5.0


def _windows_times():
    import ctypes
    from ctypes import wintypes

    idle, kernel, user = wintypes.FILETIME(), wintypes.FILETIME(), wintypes.FILETIME()
    if not ctypes.windll.kernel32.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)):
        raise OSError("GetSystemTimes failed")
    value = lambda t: (t.dwHighDateTime << 32) | t.dwLowDateTime
    # Kernel time includes idle time.
    return value(idle), value(kernel) + value(user)


def load(seconds=SAMPLE_SECONDS):
    """The load figure described above, or -1.0 if it can't be read."""
    if hasattr(os, "getloadavg"):
        try:
            return round(os.getloadavg()[0], 2)
        except OSError:
            return -1.0
    if sys.platform == "win32":
        try:
            idle0, total0 = _windows_times()
            time.sleep(seconds)
            idle1, total1 = _windows_times()
        except (OSError, AttributeError):
            return -1.0
        total = total1 - total0
        if total <= 0:
            return -1.0
        busy = 1.0 - (idle1 - idle0) / total
        return round(busy * (os.cpu_count() or 1), 2)
    return -1.0


if __name__ == "__main__":
    print(load())
