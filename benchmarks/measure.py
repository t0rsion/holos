#!/usr/bin/env python3
"""Measure wall time and peak RSS of a command.

Usage: measure.py OUTFILE CMD [ARG...]

Runs CMD with stdout redirected to OUTFILE. The target is a real file, never
a pipe: a pipe's 64 KB buffer fills up on a large diagram output and deadlocks
the child. Stderr is discarded, unless MEASURE_STDERR names a file to keep it
in. That file is also a real file, for the same reason.

MEASURE_AFFINITY, when set, is a comma-separated CPU list. The child is
pinned to it before it execs. Pinning here, instead of under taskset, keeps
argv[0] the target binary, which is what the peak RSS sampler matches on.
The parent keeps its own affinity, so the sampler never competes with the
run it times. Unset, nothing is pinned.

Wall time is time.monotonic around the process. Peak RSS is VmHWM from
/proc/PID/status, the kernel's own high-water mark, in kB. The sampling
interval is 0.5 ms for the first 20 ms and 10 ms after that, so a short-lived
process is still caught. A sample counts only once /proc/PID/cmdline shows
the target argv[0]. Before exec the child still maps the parent's image, and
ru_maxrss or a pre-exec VmHWM would report that image instead (a ~13 MB
floor as measured here). A sub-millisecond child can report 0. Linux only.

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


def cpu_list(text):
    """The CPU numbers of a comma-separated list, "0,2,12,14" style."""
    cpus = set()
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            lo, hi = part.split("-", 1)
            cpus.update(range(int(lo), int(hi) + 1))
        else:
            cpus.add(int(part))
    if not cpus:
        sys.exit(f"MEASURE_AFFINITY={text!r} names no CPU")
    return cpus


def child_pin(affinity):
    """Return a child hook that pins the process, or `None`."""
    if not affinity:
        return None
    cpus = cpu_list(affinity)

    def pin():
        os.sched_setaffinity(0, cpus)

    return pin


def run_command(outfile, cmd, errfile, pin):
    """Run and measure one command."""
    want_argv0 = os.fsencode(cmd[0])
    peak = 0
    with open(outfile, "wb") as out:
        err = open(errfile, "wb") if errfile else None
        try:
            start = time.monotonic()
            proc = subprocess.Popen(
                cmd,
                stdout=out,
                stderr=err if err else subprocess.DEVNULL,
                preexec_fn=pin,
            )
            while proc.poll() is None:
                hwm = vmhwm_kb(proc.pid, want_argv0)
                if hwm is not None and hwm > peak:
                    peak = hwm
                time.sleep(0.0005 if time.monotonic() - start < 0.02 else 0.01)
            wall = time.monotonic() - start
        finally:
            if err:
                err.close()
    return wall, peak, proc.returncode


def main():
    if len(sys.argv) < 3:
        sys.exit("usage: measure.py OUTFILE CMD [ARG...]")
    outfile, cmd = sys.argv[1], sys.argv[2:]
    errfile = os.environ.get("MEASURE_STDERR")
    pin = child_pin(os.environ.get("MEASURE_AFFINITY"))
    wall, peak, returncode = run_command(outfile, cmd, errfile, pin)
    print(f"wall_s={wall:.3f} max_rss_kb={peak}")
    sys.exit(returncode)


if __name__ == "__main__":
    main()
