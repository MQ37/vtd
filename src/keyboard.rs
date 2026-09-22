use std::path::PathBuf;

const NON_KEYBOARD_HINTS: &[&str] = &[
    "video bus",
    "sleep button",
    "power button",
    "lid switch",
    "wireless hotkeys",
    "gpio-keys",
    "wmi hotkeys",
    "output events",
    "consumer control",
];

struct Device {
    name: String,
    event_path: Option<PathBuf>,
    has_kbd_handler: bool,
}

/// Best-effort keyboard device autodetection by parsing /proc/bus/input/devices.
/// Picks the first device whose Handlers include "kbd" and an "eventN" node,
/// preferring a name containing "keyboard" and skipping known non-keyboard
/// pseudo-devices (lid switch, hotkey chords, etc). Not foolproof across all
/// hardware -- override with VTD_KEYBOARD_DEVICE if this picks the wrong one.
pub fn autodetect() -> Option<PathBuf> {
    let contents = std::fs::read_to_string("/proc/bus/input/devices").ok()?;
    let devices = parse_devices(&contents);

    let candidates: Vec<&Device> = devices
        .iter()
        .filter(|d| d.has_kbd_handler && d.event_path.is_some())
        .filter(|d| {
            let lower = d.name.to_lowercase();
            !NON_KEYBOARD_HINTS.iter().any(|hint| lower.contains(hint))
        })
        .collect();

    candidates
        .iter()
        .find(|d| d.name.to_lowercase().contains("keyboard"))
        .or_else(|| candidates.first())
        .and_then(|d| d.event_path.clone())
}

fn parse_devices(contents: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    let mut name = String::new();
    let mut event_path = None;
    let mut has_kbd_handler = false;

    for line in contents.lines() {
        if line.is_empty() {
            if !name.is_empty() {
                devices.push(Device { name: name.clone(), event_path: event_path.take(), has_kbd_handler });
            }
            name.clear();
            has_kbd_handler = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("N: Name=") {
            name = rest.trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
            has_kbd_handler = rest.split_whitespace().any(|h| h == "kbd");
            event_path = rest
                .split_whitespace()
                .find(|h| h.starts_with("event"))
                .map(|h| PathBuf::from("/dev/input").join(h));
        }
    }
    if !name.is_empty() {
        devices.push(Device { name, event_path, has_kbd_handler });
    }
    devices
}
