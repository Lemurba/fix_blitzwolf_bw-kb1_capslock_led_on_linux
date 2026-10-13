# Fix Blitzwolf BW-KB1 capslock led on linux

Fixes the **Caps Lock LED getting stuck on** with the **BlitzWolf BW-KB1**
keyboard (USB `258a:0049`) on Linux.

- [The problem](#the-problem)
- [Why it happens](#why-it-happens)
- [Solutions considered](#solutions-considered)
- [How the adopted fix works](#how-the-adopted-fix-works)
- [Requirements](#requirements)
- [Installation](#installation)
- [Building from source](#building-from-source)
- [Usage and verification](#usage-and-verification)
- [Uninstalling](#uninstalling)
- [Troubleshooting](#troubleshooting)
- [Other keyboards](#other-keyboards)
- [Development](#development)

## The problem

When Caps Lock is turned on for the first time, the LED lights up. When it is
turned off, the LED **stays on** and only resets after unplugging the keyboard.
Caps Lock itself keeps working (letters switch case correctly); only the LED is
wrong. The same keyboard works fine on Windows and Android.

## Why it happens

### Two things that are each harmless on their own

**1. The keyboard firmware only accepts the LED command while Caps Lock is held.**
The keyboard (SinoWealth controller) does not drive its Caps Lock LED by itself:
like any USB keyboard, it waits for the host to send a HID output report
(`SET_REPORT`) saying which LEDs should be on. This firmware has a bug. Once the
physical Caps Lock key has been used, it only applies LED reports that arrive
**while the Caps Lock key is held down**. Reports that arrive after the key is
released are acknowledged on the USB bus, then silently ignored.

**2. Linux turns Caps Lock off on key release, not on key press.**
On Linux, Caps Lock is handled by xkb (`libxkbcommon` in Wayland compositors,
the X server on X11). xkb's `LockMods` action, used by Caps Lock, works like
this:

- first press: Caps Lock turns on **when the key is pressed**;
- second press: Caps Lock turns off **when the key is released**.

Windows and Android toggle Caps Lock on key press both ways.

### Putting it together

| Event | Windows / Android | Linux |
|---|---|---|
| 1st press: Caps Lock on | LED report sent while key is held ✅ | LED report sent while key is held ✅ |
| 2nd press: Caps Lock off | LED report sent while key is held ✅ | LED report sent **after release** ❌ ignored |

So on Linux the "LED on" report always works and the "LED off" report never does.

### Evidence

The diagnosis was made on Bazzite (Fedora 44, kernel 7.2, KDE Plasma 6 Wayland)
by capturing USB traffic with `usbmon` and writing reports directly to
`/dev/hidraw*`:

- The kernel sends the correct reports at the right time
  (`21 09 0200 0000 0001` = `SET_REPORT`, output report, 1 byte): `02` on the
  first press, `00` right after the second release. The keyboard acknowledges
  both, but only obeys the first one.
- Before the physical key is touched, every LED report works, including fast
  on/off sequences. Turning Caps Lock on and off through a virtual (uinput)
  keyboard also works.
- After the physical key has been used, no report sent after a release works,
  from the kernel or written directly to hidraw. Only replugging the keyboard
  resets the firmware.
- An "LED off" report written to hidraw **while the Caps Lock key is held**
  works every time.

Ruled out along the way: the content of the report (`00` vs. other bit
combinations), a limit on the number of reports, a timing window after
plugging in, and other software on the system (input remappers, Steam, etc.).

## Solutions considered

### 1. Make xkb turn Caps Lock off on key press (not adopted)

`libxkbcommon` 1.8+ supports a `LockMods(..., unlockOnPress)` flag. A custom xkb
option can make Caps Lock behave like Windows, so the LED report is always sent
while the key is held.

Not adopted because:

- it depends on the compositor: it needs a recent `libxkbcommon`, a compositor
  that compiles the keymap with format v2 support, and a logout to apply (KWin
  ignored a live reload);
- the flag cannot be represented in the keymap format v1 sent to applications,
  which logs errors and may behave differently across applications and X11;
- it changes Caps Lock behavior for every keyboard, not only the faulty one.

### 2. Send the LED report at the right moment (adopted)

A small service watches the faulty keyboard and sends the "LED off" report
itself, while the key is still held. This works with any desktop environment
or display server, touches only the faulty keyboard, and needs no changes to
the keyboard configuration.

## How the adopted fix works

`blitzwolf-capsled` is a small service written in Rust with no external
dependencies.

1. It finds the keyboard's `hidraw` nodes in sysfs by USB ID: interface 0 (boot
   keyboard, which receives the LED report) and interface 1 (NKRO reports).
2. It reads the keyboard's input reports and only looks for the Caps Lock key
   (usage `0x39`).
3. It tracks the Caps Lock state from the kernel's LED in
   `/sys/class/leds/inputN::capslock`.
4. When Caps Lock is **on** and the key is **pressed**, the next release will
   turn it off, so it immediately writes an LED report with the Caps Lock bit
   cleared (keeping the other LED bits). The keyboard accepts it because the key
   is held.
5. On release, the kernel sends its own "off" report. The keyboard ignores it,
   but the LED is already off.

Other properties:

- It only **reads** the keyboard reports: it does not grab, block, change or log
  keys.
- It recovers automatically when the keyboard is unplugged and plugged back in.
- It uses about 1 MB of memory and practically no CPU.

## Requirements

| Requirement | Details |
|---|---|
| OS | x86_64 Linux with **systemd** (runs as a user service) |
| Keyboard | Wired BlitzWolf BW-KB1, USB ID `258a:0049` (check with `lsusb`) |
| Permissions | `sudo` once, to install a udev rule that gives your user access to the keyboard |
| Building from source (optional) | Rust 1.85 or newer, and a C linker (`gcc` or `clang`) |

It works with any desktop environment or display server because it acts
directly on the USB device (tested on KDE Plasma 6 with Wayland on Bazzite). It
also works on immutable distributions (Bazzite, Fedora Silverblue/Kinoite). The
release binary is static (musl) and does not depend on the glibc version.

## Installation

### Option 1: prebuilt release (recommended)

No Rust needed.

```bash
curl -LO https://github.com/Lemurba/fix_blitzwolf_bw-kb1_capslock_led_on_linux/releases/latest/download/blitzwolf-capsled-x86_64-linux.tar.gz
tar xzf blitzwolf-capsled-x86_64-linux.tar.gz
cd blitzwolf-capsled
./install.sh
```

Optionally verify the download against the checksums published with each
release:

```bash
curl -LO https://github.com/Lemurba/fix_blitzwolf_bw-kb1_capslock_led_on_linux/releases/latest/download/SHA256SUMS
sha256sum -c --ignore-missing SHA256SUMS
```

### Option 2: build from source

See [Building from source](#building-from-source). If Rust is installed,
`./install.sh` in a clone of the repository builds the binary automatically.

### What the installer does

The installer lists the connected USB keyboards and asks which one to use.
Press **Enter** for the default (`258a:0049`):

```text
Connected USB keyboards:
  1) 258a:0049  BY Tech BlitzWolf-1
Keyboard ID (list number or VID:PID) [258a:0049]:
```

Then it:

1. installs the binary to `~/.local/bin/blitzwolf-capsled` (building it with
   `cargo` if available, otherwise using the prebuilt one in `dist/`);
2. installs the udev rule `/etc/udev/rules.d/70-blitzwolf-capsled.rules`
   (asks for `sudo`), which gives the logged-in user access to the keyboard;
3. enables the user service `blitzwolf-capsled.service`, which starts
   automatically on every login.

Run the installer as a regular user, **not** with `sudo`. It only asks for your
password for the udev rule.

### Installer options

| Option | Description |
|---|---|
| `--device VID:PID` | USB ID of the keyboard, as shown by `lsusb` (`ID 258a:0049`). Default: `258a:0049` |
| `--yes` | do not ask anything; use `--device` or the default |
| `--no-udev` | do not install the udev rule (no `sudo`). Only use it if the keyboard's `hidraw` node is already accessible to your user |
| `--help` | show help |

Non-interactive example:

```bash
./install.sh --device 258a:0049 --yes
```

## Building from source

### 1. Install the build tools

Install Rust with [rustup](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

You also need a C linker, which Rust uses to link the binary:

| Distribution | Command |
|---|---|
| Debian / Ubuntu | `sudo apt install build-essential` |
| Fedora | `sudo dnf install gcc` |
| Arch | `sudo pacman -S base-devel` |
| openSUSE | `sudo zypper install gcc` |

On immutable distributions (Bazzite, Silverblue, Kinoite), install rustup in
your home directory as above, or build inside a toolbox/distrobox container.
When building in a container, prefer the static build (step 3) so the binary
does not depend on the container's glibc.

### 2. Clone and build

```bash
git clone https://github.com/Lemurba/fix_blitzwolf_bw-kb1_capslock_led_on_linux.git
cd fix_blitzwolf_bw-kb1_capslock_led_on_linux
cargo test
cargo build --release
```

The binary is written to `target/release/blitzwolf-capsled`. Try it in the
foreground (stop it with Ctrl+C):

```bash
./target/release/blitzwolf-capsled --device 258a:0049
```

If it prints `Permission denied`, install the udev rule first (see step 4).

### 3. Optional: static binary

A static binary runs on any x86_64 Linux distribution, regardless of its glibc
version:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

The binary is written to
`target/x86_64-unknown-linux-musl/release/blitzwolf-capsled`. `./install.sh`
uses this target automatically when it is installed, and `./package.sh` builds
the same archive that is published in the releases.

### 4. Install

The easiest way is to let the installer use your build:

```bash
./install.sh
```

Or install everything by hand:

```bash
# binary
install -Dm755 target/release/blitzwolf-capsled ~/.local/bin/blitzwolf-capsled

# udev rule (replace the IDs if you use another keyboard)
sed -e 's/@VID@/258a/g' -e 's/@PID@/0049/g' packaging/70-blitzwolf-capsled.rules.in \
    | sudo tee /etc/udev/rules.d/70-blitzwolf-capsled.rules >/dev/null
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw --action=change

# user service
mkdir -p ~/.config/systemd/user
sed -e 's/@VID@/258a/g' -e 's/@PID@/0049/g' packaging/blitzwolf-capsled.service.in \
    > ~/.config/systemd/user/blitzwolf-capsled.service
systemctl --user daemon-reload
systemctl --user enable --now blitzwolf-capsled.service
```

## Usage and verification

Nothing else is needed after installing: press Caps Lock a few times and the
LED should turn on and off normally.

```bash
systemctl --user status blitzwolf-capsled      # service status
journalctl --user -u blitzwolf-capsled -b      # log
```

A healthy log looks like:

```text
device: 258a:0049
watching /dev/hidraw5 /dev/hidraw6 leds=/sys/class/leds/input70
```

The binary can also be run directly:

```bash
blitzwolf-capsled --device 258a:0049
blitzwolf-capsled --help
blitzwolf-capsled --version
```

## Uninstalling

From the release folder or the repository:

```bash
./uninstall.sh
```

This removes the service, the binary and the udev rule (asks for `sudo` for the
rule).

## Troubleshooting

| Symptom | What to do |
|---|---|
| Log shows `keyboard not found, waiting...` | Check the ID with `lsusb` and reinstall with `./install.sh --device VID:PID` |
| Log shows `failed to open the keyboard (Permission denied ...)` | The udev rule is missing. Run `./install.sh` without `--no-udev` and replug the keyboard |
| LED still stuck right after installing | Replug the keyboard once: the firmware may have been stuck from before the install |
| LED on after turning Caps Lock off from another keyboard or from software | Known limitation: the firmware only accepts the report while the physical Caps Lock key is held. Press Caps Lock twice or replug the keyboard |
| Service does not start on login | `systemctl --user enable --now blitzwolf-capsled` |

## Other keyboards

`--device` lets you use the service with other keyboards that have the same
firmware bug. The program expects:

- USB interface 0 using the *boot keyboard* format (1-byte LED report);
- optionally, NKRO reports on interface 1 with report ID 4, as on the BW-KB1.

If it works with another model, please open an issue saying which one.

## Development

```bash
cargo test                 # unit tests
cargo build --release      # binary in target/release/
./package.sh               # builds blitzwolf-capsled-x86_64-linux.tar.gz (static binary)
```

Layout:

```text
src/main.rs                               the service
packaging/blitzwolf-capsled.service.in    systemd user service template
packaging/70-blitzwolf-capsled.rules.in   udev rule template
install.sh / uninstall.sh                 install and uninstall
package.sh                                builds the release archive
```

## License

[MIT](LICENSE)
