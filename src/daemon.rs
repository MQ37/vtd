use crate::config::Config;
use crate::keyboard;
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const EV_KEY: u16 = 1;
const EVENT_SIZE: usize = 24; // struct input_event on 64-bit Linux: 2x i64 + 2x u16 + i32
const RELEASE_DEBOUNCE: Duration = Duration::from_millis(500);
const MIN_RECORDING: Duration = Duration::from_millis(300);

enum Msg {
    KeyPulse,
    Tick,
}

pub fn run(cfg: Config) {
    let keyboard_device = match cfg.keyboard_device.clone().or_else(keyboard::autodetect) {
        Some(p) => p,
        None => {
            eprintln!(
                "vtd: could not autodetect a keyboard device; set VTD_KEYBOARD_DEVICE=/dev/input/eventN"
            );
            std::process::exit(1);
        }
    };

    eprintln!(
        "vtd: keyboard={:?} trigger_key={} mic_target={:?} whisper_bin={:?} model={:?}",
        keyboard_device, cfg.trigger_key, cfg.mic_target, cfg.whisper_bin, cfg.whisper_model
    );

    let (tx, rx) = mpsc::channel::<Msg>();

    let reader_tx = tx.clone();
    let trigger_key = cfg.trigger_key;
    std::thread::spawn(move || {
        if let Err(e) = read_keyboard(&keyboard_device, trigger_key, reader_tx) {
            eprintln!("vtd: keyboard reader error: {e}");
            std::process::exit(1);
        }
    });

    // ticker thread just to guarantee we periodically re-check even under load
    let ticker_tx = tx.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        if ticker_tx.send(Msg::Tick).is_err() {
            break;
        }
    });
    drop(tx);

    let mut recording: Option<(Child, PathBuf, Instant)> = None;
    let mut last_pulse: Option<Instant> = None;

    for msg in rx {
        match msg {
            Msg::KeyPulse => {
                last_pulse = Some(Instant::now());
                if recording.is_none() {
                    match start_recording(cfg.mic_target.as_deref()) {
                        Ok((child, path)) => {
                            eprintln!("vtd: recording started -> {:?}", path);
                            recording = Some((child, path, Instant::now()));
                        }
                        Err(e) => eprintln!("vtd: failed to start recording: {e}"),
                    }
                }
            }
            Msg::Tick => {
                let should_stop = match (&recording, last_pulse) {
                    (Some(_), Some(lp)) => lp.elapsed() > RELEASE_DEBOUNCE,
                    _ => false,
                };
                if should_stop {
                    if let Some((child, path, started)) = recording.take() {
                        let duration = started.elapsed();
                        stop_recording(child);
                        last_pulse = None;
                        if duration < MIN_RECORDING {
                            eprintln!("vtd: recording too short ({:?}), discarding", duration);
                            let _ = fs::remove_file(&path);
                            continue;
                        }
                        eprintln!("vtd: recording stopped, duration={:?}", duration);
                        process_and_type(&cfg, &path);
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }
}

fn read_keyboard(path: &PathBuf, trigger_key: u16, tx: mpsc::Sender<Msg>) -> std::io::Result<()> {
    let mut f = File::open(path)?;
    let mut buf = [0u8; EVENT_SIZE];
    loop {
        f.read_exact(&mut buf)?;
        let ev_type = u16::from_ne_bytes([buf[16], buf[17]]);
        let code = u16::from_ne_bytes([buf[18], buf[19]]);
        if ev_type == EV_KEY && code == trigger_key {
            let _ = tx.send(Msg::KeyPulse);
        }
    }
}

fn start_recording(mic_target: Option<&str>) -> std::io::Result<(Child, PathBuf)> {
    let path = std::env::temp_dir().join(format!("vtd-{}.wav", std::process::id()));
    let mut cmd = Command::new("pw-record");
    cmd.arg("--rate").arg("16000").arg("--channels").arg("1");
    if let Some(t) = mic_target {
        cmd.arg("--target").arg(t);
    }
    cmd.arg(&path);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn()?;
    Ok((child, path))
}

fn stop_recording(mut child: Child) {
    unsafe {
        libc_kill(child.id() as i32, 15 /* SIGTERM */);
    }
    let _ = child.wait();
}

unsafe fn libc_kill(pid: i32, sig: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let _ = unsafe { kill(pid, sig) };
}

fn process_and_type(cfg: &Config, wav_path: &PathBuf) {
    let mut command = Command::new(&cfg.whisper_bin);
    command
        .arg("-m").arg(&cfg.whisper_model)
        .arg("-f").arg(wav_path)
        .arg("-nt") // no timestamps
        .arg("-np"); // no progress
    if let Some(ld_path) = &cfg.whisper_ld_library_path {
        command.env("LD_LIBRARY_PATH", ld_path);
    }
    let output = command.output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            eprintln!("vtd: whisper-cli failed to run: {e}");
            return;
        }
    };

    if !output.status.success() {
        eprintln!("vtd: whisper-cli exited with {:?}: {}", output.status, String::from_utf8_lossy(&output.stderr));
        return;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        eprintln!("vtd: empty transcription, nothing to type");
        return;
    }
    eprintln!("vtd: transcribed: {text:?}");

    let status = Command::new("ydotool")
        .arg("type")
        .arg("--")
        .arg(text)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("vtd: ydotool exited with {:?}", s),
        Err(e) => eprintln!("vtd: failed to run ydotool: {e}"),
    }
}
