# Activity detection

`linkctl` refuses to move or reconfigure a camera that is not in use. This
document describes how "in use" is determined and where the approach falls
short.

## Definition

> The camera is **active** when another process currently holds one of its
> video nodes open.

Gimbal position is deliberately *not* used: a Link 2 pointing at the desk is
not proof that it is parked, and `pan_absolute`/`tilt_absolute` reads may
return the last commanded value rather than the physical position.

Two questions are answered separately, because they cost wildly different
amounts:

| question                     | used by                        | how |
|------------------------------|--------------------------------|-----|
| may this command run?        | the guard on every write       | `is_active`: O(1) sysfs reading when it says yes, `/proc` scan otherwise |
| is the camera in use?        | `status`, `info`               | `quick_in_use`: O(1) sysfs reading, falling back to a scan |
| *who* is using it?           | `status --holders`             | full `/proc` scan |

`status` reports which one it used in its `detection` field, so a caller can
tell an exact answer from a cheap one. The holders list is opt-in precisely
because producing it is the expensive part.

## Why not V4L2 or sysfs?

There is no kernel interface that reports the open count of a V4L2 device:

* `VIDIOC_QUERYCAP` and the control ioctls are stateless.
* `/sys/class/video4linux/videoN` exposes `name`, `index`, `dev` and the
  USB parent, but nothing about users.
* The `uvcvideo` driver keeps a `users` counter internally (it starts the
  status interrupt URB on first open) but does not export it.
* `/dev/mediaN` topology is static.
* Using `VIDIOC_REQBUFS`/`VIDIOC_STREAMON` to probe whether the stream is
  busy would itself be an intrusive operation and could start streaming.

Consequently `linkctl` scans `/proc`.

## Implementation (`src/camera/activity.rs`)

1. Compute the `st_rdev` of every video node belonging to the camera
   (`/dev/video0` **and** `/dev/video1` on the Link 2; the metadata node is
   included because anything holding it keeps the camera powered).
2. Iterate `/proc/<pid>` for numeric directory names, skipping our own pid.
3. For each `/proc/<pid>/fd/<n>` call `stat(2)` on the entry. This follows
   the magic link to the open file and yields its `st_mode` and `st_rdev`.
4. If the target is a character device whose `st_rdev` matches, the process
   is a *holder* and the camera is active.

Comparing device numbers rather than the `/dev/videoN` string means renamed
paths, bind mounts and containers (which see the same `rdev`) are handled.
Deleted or replaced device nodes never match by accident.

`linkctl` opens the control node only *after* the scan (or, for commands
that validate arguments first, with our own pid excluded), so its own file
descriptor never counts as activity. The preview player spawned by
`linkctl preview` is a separate process and therefore does count, which is
the intended behaviour.

Errors are handled per process: `EACCES` or `ESRCH` on one `/proc/<pid>/fd`
increments a "skipped" counter (visible with `--verbose`) without aborting
the scan.

## Limitations

* **Other users' processes are invisible.** `/proc/<pid>/fd` is only
  readable for processes of the same user (or with `CAP_SYS_PTRACE`). A
  camera opened by a different user, or by a system service such as a
  container runtime running as root, is reported as inactive. In that case
  use `--force` deliberately.
* **`hidepid`.** Systems mounting `/proc` with `hidepid=1|2` hide other
  processes entirely; the same rule applies.
* **Open is not streaming.** A process that holds the node open without
  streaming (some capture frameworks probe devices and keep the descriptor)
  counts as active. In practice PipeWire/WirePlumber close V4L2 nodes when no
  client is streaming, so this has not been an issue on the development
  machine.
* **Cost.** The scan touches every `/proc/<pid>/fd` entry of the user, so it
  is O(file descriptors open on the whole machine). On a typical desktop that
  is a few milliseconds. It is not bounded by anything `linkctl` controls: a
  single unrelated process leaking descriptors drags every invocation down
  with it. On the development machine, with another program leaking fds, the
  cost was measured at:

  | fds open on the machine | `status` |
  |-------------------------|----------|
  | 766 897                 | 2.3 s    |
  | 1 525 257               | 3.8 s    |
  | 1 707 630               | 4.9 s    |

  Essentially all of that is the scan: `devices`, which does the same device
  discovery without scanning, runs in 2 ms. This is why the scan has a
  deadline (`SCAN_BUDGET`) and why commands that only need the state string
  do not use it.
* **Race.** A process that closes the device between the scan and the
  control write is not detected. The window is microseconds and the
  consequence is a single command reaching a parked camera, which is
  harmless.

## Why the scan cannot be made fast

Proving that the camera is idle is a universal negative: it requires ruling
out every descriptor on the system. Several alternatives were measured on a
Link 2 and all were rejected.

**Cheaper syscalls do not help.** The cost is the per-descriptor syscall
itself, not the operation, so there is no constant factor worth chasing
(measured over 767 k descriptors):

| approach                     | per fd  | total   |
|------------------------------|---------|---------|
| `stat` on the full path      | 2.8 us  | 2.13 s  |
| `readlink`                   | 3.5 us  | 2.71 s  |
| `fstatat` relative to the dir| 2.2 us  | 1.70 s  |

**`bAlternateSetting` is useless on this device.** The usual way to spot a
streaming UVC camera is a non-zero alternate setting on the streaming
interface. The Link 2 keeps it at `0` even while delivering 30 fps, because
it streams over a bulk endpoint rather than isochronous ones. Verified with
`ffmpeg` capturing 301 frames while the attribute stayed `0`.

**Per-interface `power/` directories are empty**, so the video interface
cannot be distinguished from the audio ones that way.

**USB runtime PM answers a different question.** `power/runtime_status` on the
USB *device* is O(1) and does track use, but at device granularity:

| situation                         | `runtime_status` | holders scan |
|-----------------------------------|------------------|--------------|
| nothing using the camera          | `suspended`      | inactive     |
| node open, not streaming          | `suspended`      | **active**   |
| video streaming                   | `active`         | active       |
| only the built-in microphone open | **`active`**     | inactive     |

The second row is the important one: the gimbal does respond when a process
holds the node open without streaming (measured: pan moved from -17 deg to
-12 deg in that state), so treating `suspended` as "idle" would refuse moves
that would have worked. That is why `is_active` only takes this signal in the
positive direction, where it cannot cause a wrong refusal.

Two further caveats apply to the signal:

* It is meaningless when `power/control` is `on`, which disables autosuspend
  and pins the status at `active`. `usb_device_in_use` declines to answer then.
* **`linkctl` wakes the device itself.** Any command issuing USB control
  transfers (`info`, `tracking status`, any control read or write) resumes the
  device, and it takes roughly 2-4 s to autosuspend again. A `status` run
  immediately after one of those will report the device as in use. Callers
  polling for state should read `status` first. Transitions were measured at
  under 0.6 s to become `active` and 3-4 s to fall back to `suspended`.

## Tests

The scanner is exercised against a synthetic `/proc` tree in unit tests
(`scan_detects_holder_in_synthetic_tree`), against the real `/proc` to
confirm that our own pid is never reported, and its helper functions
(`parse_pid`, char-device detection) are tested directly.
