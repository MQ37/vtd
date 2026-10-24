use crate::config::Config;
use crate::daemon;
use crate::heavy;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `vtd transcribe FILE [-o|--output OUT] [--heavy]`: one-shot file transcription using the
/// configured backend (the Phonon server must be running, or a whisper.cpp build present), or
/// with `--heavy` the sandboxed GPU whisper large-v3.
pub fn run(args: &[String]) {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut use_heavy = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                let Some(v) = args.get(i + 1) else { die(&format!("{} requires a file name", args[i])) };
                output = Some(PathBuf::from(v));
                i += 1;
            }
            "--heavy" => use_heavy = true,
            a if a.starts_with('-') && a.len() > 1 => die(&format!("unknown transcribe option {a:?}")),
            a if input.is_none() => input = Some(PathBuf::from(a)),
            _ => die("expected exactly one audio file"),
        }
        i += 1;
    }
    let Some(input) = input else { die("usage: vtd transcribe FILE [-o OUT.txt] [--heavy]") };
    if !input.is_file() {
        die(&format!("{} is not a file", input.display()));
    }

    // Phonon and whisper-cli want 16 kHz mono WAV; ffmpeg converts anything else. The
    // temporary WAV lives in the RAM-backed runtime dir when there is one.
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let wav = dir.join(format!("vtd-transcribe-{}.wav", std::process::id()));
    if let Err(e) = convert(&input, &wav) {
        die(&e);
    }
    let text = if use_heavy {
        heavy::transcribe(&wav).map_err(|e| eprintln!("vtd: {e}")).ok().filter(|t| !t.is_empty())
    } else {
        daemon::transcribe(&Config::from_env(), &wav)
    };
    let _ = std::fs::remove_file(&wav);
    let Some(text) = text else { std::process::exit(1) };

    match output {
        Some(out) => {
            if let Err(e) = std::fs::write(&out, format!("{text}\n")) {
                die(&format!("cannot write {}: {e}", out.display()));
            }
        }
        None => println!("{text}"),
    }
}

fn convert(input: &Path, wav: &Path) -> Result<(), String> {
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", "-f", "wav"])
        .arg(wav)
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("failed to run ffmpeg (needed to read {}): {e}", input.display()))?;
    if status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(wav);
        Err(format!("ffmpeg could not read {} as audio", input.display()))
    }
}

fn die(msg: &str) -> ! {
    eprintln!("vtd: {msg}");
    std::process::exit(1);
}
