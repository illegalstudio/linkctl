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
* **Cost.** The scan touches every `/proc/<pid>/fd` directory of the user.
  On a typical desktop this is a few milliseconds and is well within the
  budget for keybinding use.
* **Race.** A process that closes the device between the scan and the
  control write is not detected. The window is microseconds and the
  consequence is a single command reaching a parked camera, which is
  harmless.

## Tests

The scanner is exercised against a synthetic `/proc` tree in unit tests
(`scan_detects_holder_in_synthetic_tree`), against the real `/proc` to
confirm that our own pid is never reported, and its helper functions
(`parse_pid`, char-device detection) are tested directly.
