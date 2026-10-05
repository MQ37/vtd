use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Phonon,
    Whisper,
}

pub struct Config {
    pub backend: Backend,
    pub keyboard_device: Option<PathBuf>,
    pub mic_target: Option<String>,
    pub whisper_bin: PathBuf,
    pub whisper_model: PathBuf,
    pub whisper_ld_library_path: Option<String>,
    pub phonon_socket: PathBuf,
    pub trigger_key: u16,
    pub key_delay_ms: u32,
    pub key_hold_ms: u32,
}

/// `VTD_BACKEND` (phonon unless set to whisper); also the default for `vtd install`.
pub fn backend_from_env() -> Backend {
    match std::env::var("VTD_BACKEND").as_deref() {
        Ok("whisper") => Backend::Whisper,
        Ok("phonon") | Err(_) => Backend::Phonon,
        Ok(other) => {
            eprintln!("vtd: unknown VTD_BACKEND {other:?} (expected \"phonon\" or \"whisper\")");
            std::process::exit(1);
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            backend: backend_from_env(),
            keyboard_device: std::env::var("VTD_KEYBOARD_DEVICE").ok().map(PathBuf::from),
            mic_target: std::env::var("VTD_MIC_TARGET").ok(),
            whisper_bin: std::env::var("VTD_WHISPER_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|_| data_dir().join("whisper.cpp/build/bin/whisper-cli")),
            whisper_model: std::env::var("VTD_WHISPER_MODEL")
                .map(PathBuf::from)
                .unwrap_or_else(|_| models_dir().join("ggml-large-v3-turbo.bin")),
            whisper_ld_library_path: std::env::var("VTD_WHISPER_LD_LIBRARY_PATH").ok(),
            phonon_socket: std::env::var("VTD_PHONON_SOCKET")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_phonon_socket()),
            trigger_key: std::env::var("VTD_TRIGGER_KEY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(KEY_RIGHTALT),
            key_delay_ms: env_u32("VTD_KEY_DELAY", DEFAULT_KEY_DELAY_MS),
            key_hold_ms: env_u32("VTD_KEY_HOLD", DEFAULT_KEY_HOLD_MS),
        }
    }
}

/// Where the sandboxed Phonon server listens: `$XDG_RUNTIME_DIR/vtd/phonon.sock`
/// (RAM-backed, owner-only), matching the systemd unit's `%t/vtd`.
pub fn default_phonon_socket() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
        PathBuf::from(format!("/run/user/{uid}"))
    });
    runtime.join("vtd/phonon.sock")
}

/// ydotool's own defaults are 20 ms delay + 20 ms hold per key (~25 chars/s); these are
/// ~4x faster while staying conservative enough for terminals, browsers and Electron apps.
const DEFAULT_KEY_DELAY_MS: u32 = 5;
const DEFAULT_KEY_HOLD_MS: u32 = 5;

fn env_u32(name: &str, default: u32) -> u32 {
    match std::env::var(name) {
        Ok(v) => v.parse().unwrap_or_else(|_| {
            eprintln!("vtd: ignoring invalid {name}={v:?} (expected milliseconds), using {default}");
            default
        }),
        Err(_) => default,
    }
}

/// KEY_RIGHTALT from linux/input-event-codes.h
pub const KEY_RIGHTALT: u16 = 100;

pub fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

/// XDG data dir for vtd's own files (whisper.cpp checkout + build, models).
pub fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/share"))
        .join("vtd")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn systemd_user_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
        .join("systemd/user")
}
