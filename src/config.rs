use std::path::PathBuf;

pub struct Config {
    pub keyboard_device: Option<PathBuf>,
    pub mic_target: Option<String>,
    pub whisper_bin: PathBuf,
    pub whisper_model: PathBuf,
    pub whisper_ld_library_path: Option<String>,
    pub trigger_key: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            keyboard_device: std::env::var("VTD_KEYBOARD_DEVICE").ok().map(PathBuf::from),
            mic_target: std::env::var("VTD_MIC_TARGET").ok(),
            whisper_bin: std::env::var("VTD_WHISPER_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|_| data_dir().join("whisper.cpp/build/bin/whisper-cli")),
            whisper_model: std::env::var("VTD_WHISPER_MODEL")
                .map(PathBuf::from)
                .unwrap_or_else(|_| models_dir().join("ggml-large-v3-turbo.bin")),
            whisper_ld_library_path: std::env::var("VTD_WHISPER_LD_LIBRARY_PATH").ok(),
            trigger_key: std::env::var("VTD_TRIGGER_KEY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(KEY_RIGHTALT),
        }
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
