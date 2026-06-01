#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use notify_rust::Notification;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use tray_item::{IconSource, TrayItem};

const LATENCY: &str = "125";
const PIPE: &str = "/tmp/scrcpy_pipe";

struct PhoneMicState {
    module_id: Option<u32>,
    parec: Option<Child>,
    scrcpy: Option<Child>,
}

impl PhoneMicState {
    fn new() -> Self {
        Self { module_id: None, parec: None, scrcpy: None }
    }

    fn is_active(&self) -> bool {
        self.scrcpy.is_some()
    }

    fn start(&mut self) -> Result<(), String> {
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-pipe-source",
                "source_name=Scrcpy",
                "channels=2",
                "format=16",
                "rate=48000",
                &format!("file={}", PIPE),
            ])
            .output()
            .map_err(|e| format!("pactl failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("pactl load-module failed: {}", stderr));
        }

        let module_id: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .map_err(|e| format!("invalid module id: {}", e))?;
        self.module_id = Some(module_id);

        let parec = Command::new("parec")
            .args(["--fix-rate", "-d", "Scrcpy", "--raw"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("parec failed: {}", e))?;
        self.parec = Some(parec);

        let scrcpy = Command::new("scrcpy")
            .args([
                "--no-video",
                "--no-window",
                "--no-playback",
                "--audio-source=mic",
                "--audio-codec=raw",
                "--record-format=wav",
                &format!("--record={}", PIPE),
                &format!("--audio-buffer={}", LATENCY),
                "--audio-output-buffer=10",
            ])
            .spawn()
            .map_err(|e| format!("scrcpy failed: {}", e))?;
        self.scrcpy = Some(scrcpy);

        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.scrcpy.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut child) = self.parec.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(id) = self.module_id.take() {
            let _ = Command::new("pactl")
                .args(["unload-module", &id.to_string()])
                .output();
        }
    }
}

enum TrayMessage {
    Toggle,
    Quit,
}

fn get_icon() -> IconSource {
    #[cfg(target_os = "windows")]
    return IconSource::Resource("phone-mic");
    #[cfg(not(target_os = "windows"))]
    return IconSource::Resource("audio-input-microphone");
}

fn main() {
    let instance = single_instance::SingleInstance::new("phone-mic").unwrap();
    if !instance.is_single() {
        eprintln!("Another instance of phone-mic is already running");
        std::process::exit(1);
    }

    let mut tray = TrayItem::new("Phone Mic", get_icon()).unwrap();
    tray.add_label("Phone Mic").unwrap();

    let state = Mutex::new(PhoneMicState::new());
    let (tx, rx) = mpsc::sync_channel(1);

    let toggle_tx = tx.clone();
    tray.add_menu_item("Toggle Phone Mic", move || {
        let _ = toggle_tx.send(TrayMessage::Toggle);
    })
    .unwrap();

    #[cfg(target_os = "windows")]
    tray.inner_mut().add_separator().unwrap();

    let quit_tx = tx.clone();
    tray.add_menu_item("Quit", move || {
        let _ = quit_tx.send(TrayMessage::Quit);
    })
    .unwrap();

    loop {
        match rx.recv() {
            Ok(TrayMessage::Toggle) => {
                let mut s = state.lock().unwrap();
                if s.is_active() {
                    s.stop();
                    let _ = Notification::new()
                        .appname("Phone Mic")
                        .summary("Phone Mic")
                        .body("Deactivated")
                        .show();
                } else if let Err(e) = s.start() {
                    eprintln!("phone-mic error: {}", e);
                    let _ = Notification::new()
                        .appname("Phone Mic")
                        .summary("Phone Mic Error")
                        .body(&e)
                        .show();
                } else {
                    let _ = Notification::new()
                        .appname("Phone Mic")
                        .summary("Phone Mic")
                        .body("Activated")
                        .show();
                }
            }
            Ok(TrayMessage::Quit) => {
                let mut s = state.lock().unwrap();
                s.stop();
                std::process::exit(0);
            }
            Err(_) => break,
        }
    }
}
