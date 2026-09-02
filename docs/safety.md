# Safety

`linkctl` is designed so that an accidental invocation can never surprise
the user with physical movement, and so that no code path can put the
camera into a state that requires a power cycle or worse.

## Two kinds of controls

### Standard V4L2 controls (the normal path)

Pan, tilt, zoom, focus, white balance, brightness, contrast, saturation,
hue and sharpness are all standard UVC controls that the Linux `uvcvideo`
driver exposes as V4L2 controls. `linkctl` uses them through `VIDIOC_QUERYCTRL`,
`VIDIOC_G_CTRL` and `VIDIOC_S_CTRL` on the video node.

These are the same operations `v4l2-ctl` performs and have been confirmed
to work on the development Insta360 Link 2 (kernel 7.1). Values are
validated against the range the *device* reports before every write; nothing
is hard-coded.

Some third-party projects report that `pan_absolute`/`tilt_absolute` SET
fails on Link 2. That is not what the development hardware does. If a future
firmware or kernel behaves differently, the failure is a plain `EINVAL`/`EIO`
from the ioctl and is reported as a device I/O error; no fallback to raw USB
is attempted.

### Vendor extension unit (XU) controls (the experimental path)

Features that have no standard UVC control (AI tracking, DeskView, Whiteboard,
Overhead, privacy, gestures, HDR) live in Insta360's proprietary extension
units. `linkctl` reaches them through the kernel's `UVCIOC_CTRL_QUERY` ioctl
on the same video node. This has three consequences:

* the kernel driver stays bound at all times;
* other applications keep working while `linkctl` talks to the camera;
* the driver serialises our requests with regular control traffic.

Only one vendor control is written today: AI tracking on/off
(unit 11, selector 0x02, 1 byte). Every vendor write goes through
`V4l2Device::xu_write`, which requires a static `XuControl` descriptor and
enforces, in order:

1. the payload length equals the documented constant;
2. the device's `GET_LEN` equals the documented constant;
3. the device's `GET_INFO` advertises SET support;
4. the unit id carries the expected GUID in the USB descriptors
   (checked by the `tracking` command before any write);
5. the payload was obtained by reading the current value first
   (read-modify-write);
6. the value is read back afterwards and a mismatch is reported.

Read-only vendor queries (`GET_LEN`, `GET_INFO`, `GET_CUR`) are used for
diagnostics (`info`, `status`, `tracking status`). The unit-9 "AI mode"
payload is read with the length the device reports because it differs
between firmware versions (61 bytes documented elsewhere, 60 on the
development camera). It is never written.

## Why the kernel driver is never detached

Some projects use libusb with `detach_kernel_driver` to send UVC control
transfers directly. Public reports on the Link 2 describe that repeated
detach/rebind cycles, especially combined with rapid vendor writes, can leave
the firmware unresponsive: the USB device stays enumerated but `/dev/video*`
disappears or the camera stops answering until it is unplugged.

`linkctl` therefore:

* never calls libusb at all (the crate is not a dependency);
* never unbinds or rebinds `uvcvideo`;
* never resets the USB device;
* never touches interface 1 or the metadata node for control.

### Format changes

`linkctl resolution` uses `VIDIOC_S_FMT` / `VIDIOC_S_PARM`. These make the
driver negotiate a format with the camera's streaming interface (a UVC
probe) but do not start streaming, so the gimbal stays parked; the
development camera stayed reported as inactive across format changes. The
driver refuses the change with `EBUSY` while another process streams, which
`linkctl` reports as "camera is in use" (exit code 13). Requested sizes and
rates are validated against what the camera enumerates before the ioctl.

## Inactive camera guard

Every command that changes camera state checks whether another process has
the camera open (see `activity-detection.md`) and refuses with exit code 5
otherwise. `--force` is the only way past the guard and must be typed
explicitly. Read commands and `preview` are always allowed.

## What `linkctl` deliberately refuses to do

* firmware or bootloader operations of any kind;
* EEPROM or persistent configuration writes;
* writes to unknown selectors, units or payload lengths;
* generic extension-unit scanning or fuzzing;
* writes to the 60/61-byte AI mode payload (unit 9, selector 0x02);
* constructing vendor payloads from scratch when a read is possible;
* detaching the kernel driver or resetting the USB device;
* changing udev rules, groups or ACLs on the user's behalf;
* installing software (e.g. FFmpeg for `preview`).

If a future `linkctl debug` subcommand is ever added for low-level
exploration, it must remain read-only unless the user passes an explicit
opt-in flag, and it must never appear in the default help output paths that
scripts use.

## Known soft-hang behaviour (from public reports)

* Rapid successive AI-mode writes can leave unit 9 selector 0x02 reporting
  `0xFF` (transition) indefinitely.
* `libusb_reset_device` after such a state has been observed to leave the
  mode stuck.
* Recovery in all reported cases was a USB unplug/replug or, as root,
  unbinding and rebinding `uvcvideo` for both interfaces.

`linkctl` avoids the triggers entirely. If the camera nevertheless stops
exposing `/dev/video*`, unplug it and plug it back in.
