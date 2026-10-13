//! Works around a firmware bug in the BlitzWolf BW-KB1 keyboard (USB 258a:0049).
//!
//! Once the physical Caps Lock key has been used, the firmware only accepts LED
//! commands that arrive WHILE the Caps Lock key is held down. Linux (xkb) turns
//! Caps Lock off when the key is RELEASED, so the "LED off" command is ignored
//! and the LED stays on.
//!
//! This service watches the Caps Lock key through hidraw (it only reads the
//! keyboard reports and never blocks anything). When Caps Lock is on and the
//! key is pressed, it sends "LED off" right away, while the key is still held.
//!
//! Usage: blitzwolf-capsled [--device VID:PID]   (default: 258a:0049)

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const DEFAULT_DEVICE: &str = "258a:0049";
const CAPS_USAGE: u8 = 0x39;
const NKRO_REPORT_ID: u8 = 4;
const NKRO_FIRST_USAGE: u8 = 0x04;
const LED_BITS: [(&str, u8); 5] = [
    ("numlock", 0x01),
    ("capslock", 0x02),
    ("scrolllock", 0x04),
    ("compose", 0x08),
    ("kana", 0x10),
];

struct Device {
    /// hidraw node of interface 0 (boot keyboard, receives the LED report)
    if0: PathBuf,
    /// hidraw node of interface 1 (NKRO reports, used in some modes)
    if1: Option<PathBuf>,
    /// sysfs LED prefix, e.g. /sys/class/leds/input62
    leds: String,
}

#[derive(Clone, Copy)]
enum Interface {
    Boot,
    Nkro,
}

/// State shared between the reader threads.
struct State {
    leds: String,
    writer: File,
    /// Caps Lock state before the current key press (the compositor changes it during the press)
    caps_before: AtomicBool,
    held_boot: AtomicBool,
    held_nkro: AtomicBool,
}

impl State {
    fn led(&self, name: &str) -> bool {
        fs::read_to_string(format!("{}::{}/brightness", self.leds, name))
            .map(|s| s.trim() != "0")
            .unwrap_or(false)
    }

    fn held(&self, iface: Interface) -> &AtomicBool {
        match iface {
            Interface::Boot => &self.held_boot,
            Interface::Nkro => &self.held_nkro,
        }
    }

    fn any_held(&self) -> bool {
        self.held_boot.load(Ordering::SeqCst) || self.held_nkro.load(Ordering::SeqCst)
    }

    fn on_caps(&self, iface: Interface, pressed: bool) {
        let was_held = self.held(iface).swap(pressed, Ordering::SeqCst);
        if pressed && !was_held {
            // Caps Lock was on: xkb will turn it off on release. Turn the LED off now.
            if self.caps_before.load(Ordering::SeqCst) {
                let mask = LED_BITS
                    .iter()
                    .filter(|(name, _)| *name != "capslock" && self.led(name))
                    .fold(0u8, |m, (_, bit)| m | bit);
                if let Err(e) = (&self.writer).write_all(&[0, mask]) {
                    println!("failed to write LED report: {e}");
                }
            }
        } else if !pressed && was_held {
            thread::sleep(Duration::from_millis(50)); // let the compositor update the state
            self.caps_before.store(self.led("capslock"), Ordering::SeqCst);
        }
    }
}

fn find_device(hid_id: &str) -> Option<Device> {
    let mut if0 = None;
    let mut if1 = None;
    for entry in fs::read_dir("/sys/class/hidraw").ok()?.flatten() {
        let Ok(real) = fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        let real = real.to_string_lossy();
        if !real.contains(hid_id) {
            continue;
        }
        let dev = Path::new("/dev").join(entry.file_name());
        if real.contains(":1.0/") {
            if0 = Some(dev);
        } else if real.contains(":1.1/") {
            if1 = Some(dev);
        }
    }
    let mut leds = None;
    for entry in fs::read_dir("/sys/class/leds").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(prefix) = name.strip_suffix("::capslock") else {
            continue;
        };
        let Ok(real) = fs::canonicalize(entry.path()) else {
            continue;
        };
        let real = real.to_string_lossy();
        if real.contains(hid_id) && real.contains(":1.0/") {
            leds = Some(format!("/sys/class/leds/{prefix}"));
        }
    }
    Some(Device { if0: if0?, if1, leds: leds? })
}

/// Boot report: [modifiers, reserved, 6 keys].
fn caps_in_boot(data: &[u8]) -> Option<bool> {
    (data.len() >= 8).then(|| data[2..8].contains(&CAPS_USAGE))
}

/// Interface 1, report id 4: bitmap of usages starting at 0x04.
fn caps_in_nkro(data: &[u8]) -> Option<bool> {
    if data.first() != Some(&NKRO_REPORT_ID) {
        return None;
    }
    let bit = (CAPS_USAGE - NKRO_FIRST_USAGE) as usize;
    data.get(1 + bit / 8).map(|b| b & (1 << (bit % 8)) != 0)
}

fn spawn_reader(
    path: &Path,
    iface: Interface,
    state: Arc<State>,
) -> std::io::Result<thread::JoinHandle<()>> {
    let mut file = File::open(path)?;
    Ok(thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            let n = match file.read(&mut buf) {
                Ok(0) | Err(_) => return, // keyboard disconnected
                Ok(n) => n,
            };
            let pressed = match iface {
                Interface::Boot => caps_in_boot(&buf[..n]),
                Interface::Nkro => caps_in_nkro(&buf[..n]),
            };
            if let Some(pressed) = pressed {
                state.on_caps(iface, pressed);
            }
        }
    }))
}

/// Watches the keyboard until it is disconnected.
fn run(dev: Device) -> std::io::Result<()> {
    let writer = OpenOptions::new().write(true).open(&dev.if0)?;
    let state = Arc::new(State {
        caps_before: AtomicBool::new(false),
        held_boot: AtomicBool::new(false),
        held_nkro: AtomicBool::new(false),
        leds: dev.leds,
        writer,
    });
    state.caps_before.store(state.led("capslock"), Ordering::SeqCst);

    let boot = spawn_reader(&dev.if0, Interface::Boot, state.clone())?;
    // The NKRO thread is optional and exits on its own when the device goes away.
    let nkro = dev.if1.as_deref().map(|p| spawn_reader(p, Interface::Nkro, state.clone()));
    println!(
        "watching {} {} leds={}",
        dev.if0.display(),
        match &nkro {
            Some(Ok(_)) => dev.if1.as_ref().unwrap().display().to_string(),
            Some(Err(e)) => format!("(interface 1 unavailable: {e})"),
            None => String::new(),
        },
        state.leds
    );

    // Track Caps Lock changes made outside this keyboard (another keyboard, software).
    while !boot.is_finished() {
        thread::sleep(Duration::from_millis(500));
        if !state.any_held() {
            state.caps_before.store(state.led("capslock"), Ordering::SeqCst);
        }
    }
    Ok(())
}

/// Converts "258a:0049" into the fragment used in sysfs HID paths
/// (e.g. .../0003:258A:0049.000E/...).
fn parse_device(s: &str) -> Result<String, String> {
    let err = || format!("invalid device ID: '{s}' (expected VID:PID, e.g. 258a:0049)");
    let (vid, pid) = s.split_once(':').ok_or_else(err)?;
    let valid = |p: &str| p.len() == 4 && p.chars().all(|c| c.is_ascii_hexdigit());
    if !valid(vid) || !valid(pid) {
        return Err(err());
    }
    Ok(format!(":{}:{}.", vid.to_ascii_uppercase(), pid.to_ascii_uppercase()))
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<String, String> {
    let mut device = DEFAULT_DEVICE.to_string();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-d" | "--device" => device = args.next().ok_or("--device requires a value")?,
            "-h" | "--help" => {
                println!("usage: blitzwolf-capsled [--device VID:PID]  (default: {DEFAULT_DEVICE})");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("blitzwolf-capsled {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            _ => match arg.strip_prefix("--device=") {
                Some(v) => device = v.to_string(),
                None => return Err(format!("unknown argument: {arg}")),
            },
        }
    }
    Ok(device)
}

fn main() {
    let device = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let hid_id = parse_device(&device).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    println!("device: {device}");

    // Only log when the message changes, so waiting does not flood the log.
    let mut last = String::new();
    let mut report = |msg: String| {
        if msg != last {
            println!("{msg}");
            last = msg;
        }
    };
    loop {
        match find_device(&hid_id) {
            None => report("keyboard not found, waiting...".into()),
            Some(dev) => match run(dev) {
                Ok(()) => report("keyboard disconnected, searching again".into()),
                Err(e) => report(format!("failed to open the keyboard ({e}), retrying")),
            },
        }
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_report() {
        assert_eq!(caps_in_boot(&[0, 0, 0x39, 0, 0, 0, 0, 0]), Some(true));
        assert_eq!(caps_in_boot(&[0, 0, 0x04, 0x39, 0, 0, 0, 0]), Some(true));
        assert_eq!(caps_in_boot(&[0; 8]), Some(false));
        assert_eq!(caps_in_boot(&[0; 3]), None);
    }

    #[test]
    fn device_id() {
        assert_eq!(parse_device("258a:0049").unwrap(), ":258A:0049.");
        assert_eq!(parse_device("ABCD:ef01").unwrap(), ":ABCD:EF01.");
        assert!(parse_device("258a").is_err());
        assert!(parse_device("258a:49").is_err());
        assert!(parse_device("zzzz:0049").is_err());
        assert!(parse_device("258a:0049:1").is_err());
    }

    fn args(v: &[&str]) -> Result<String, String> {
        parse_args(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn cli_args() {
        assert_eq!(args(&[]).unwrap(), DEFAULT_DEVICE);
        assert_eq!(args(&["--device", "1234:5678"]).unwrap(), "1234:5678");
        assert_eq!(args(&["-d", "1234:5678"]).unwrap(), "1234:5678");
        assert_eq!(args(&["--device=1234:5678"]).unwrap(), "1234:5678");
        assert!(args(&["--device"]).is_err());
        assert!(args(&["--foo"]).is_err());
    }

    #[test]
    fn nkro_report() {
        let mut r = [0u8; 16];
        r[0] = NKRO_REPORT_ID;
        assert_eq!(caps_in_nkro(&r), Some(false));
        let bit = (CAPS_USAGE - NKRO_FIRST_USAGE) as usize;
        r[1 + bit / 8] |= 1 << (bit % 8);
        assert_eq!(caps_in_nkro(&r), Some(true));
        r[0] = 2;
        assert_eq!(caps_in_nkro(&r), None);
    }
}
