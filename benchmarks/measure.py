#!/usr/bin/env python3
"""Measure wall time and peak RSS of a command.

Usage: measure.py OUTFILE CMD [ARG...]

Runs CMD with stdout redirected to OUTFILE. The redirection target is a real
file, never a pipe: a pipe's 64 KB buffer fills up on large diagram outputs
and deadlocks the child. Stderr is discarded.

Wall time is time.monotonic around the process. Peak RSS is VmHWM from
/proc/PID/status (the kernel's own high-water mark, in kB), sampled every
0.5 ms for the first 20 ms and every 10 ms after, so short-lived processes
are still caught. Samples are only accepted once /proc/PID/cmdline shows the
target argv[0]: before exec the child still maps the parent's image, and
ru_maxrss/pre-exec VmHWM would report that instead (measured ~13 MB floor
here). A sub-millisecond child can report 0. Linux only.

Prints one line: "wall_s=<float> max_rss_kb=<int>". Exits with the child's
exit code.
"""

import os
import subprocess
import sys
import time


def vmhwm_kb(pid, want_argv0):
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            if f.read().split(b"\0", 1)[0] != want_argv0:
                return None
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1])
    except OSError:
        return None
    return None


def main():
    if len(sys.argv) < 3:
        sys.exit("usage: measure.py OUTFILE CMD [ARG...]")
    outfile, cmd = sys.argv[1], sys.argv[2:]
    want_argv0 = os.fsencode(cmd[0])
    peak = 0
    with open(outfile, "wb") as out:
        start = time.monotonic()
        proc = subprocess.Popen(cmd, stdout=out, stderr=subprocess.DEVNULL)
        while proc.poll() is None:
            hwm = vmhwm_kb(proc.pid, want_argv0)
            if hwm is not None and hwm > peak:
                peak = hwm
            time.sleep(0.0005 if time.monotonic() - start < 0.02 else 0.01)
        wall = time.monotonic() - start
    print(f"wall_s={wall:.3f} max_rss_kb={peak}")
    sys.exit(proc.returncode)


if __name__ == "__main__":
    main()
