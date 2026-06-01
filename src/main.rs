#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

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

fn prime_adb() {
    let _ = Command::new("adb")
        .args(["devices"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// ---------------------------------------------------------------------------
// Linux: ksni tray with dynamic menu
// ---------------------------------------------------------------------------
#[cfg(not(target_os = "windows"))]
mod tray {
    use super::*;
    use ksni;

    pub struct AppData {
        pub active: bool,
        pub tx: mpsc::Sender<TrayMessage>,
    }

    struct PhoneMicTray {
        data: Arc<Mutex<AppData>>,
    }

    impl ksni::Tray for PhoneMicTray {
        fn icon_name(&self) -> String {
            let active = self.data.lock().unwrap().active;
            if active { "media-record" } else { "audio-input-microphone" }.to_string()
        }

        fn title(&self) -> String {
            "Phone Mic".to_string()
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::*;
            let data = self.data.lock().unwrap();
            let label = if data.active { "Deactivate Phone Mic" } else { "Activate Phone Mic" };

            vec![
                StandardItem {
                    label: label.to_string(),
                    activate: Box::new(|this: &mut PhoneMicTray| {
                        let data = this.data.lock().unwrap();
                        let _ = data.tx.send(TrayMessage::Toggle);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Quit".to_string(),
                    activate: Box::new(|this: &mut PhoneMicTray| {
                        let data = this.data.lock().unwrap();
                        let _ = data.tx.send(TrayMessage::Quit);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub struct Handle {
        inner: ksni::Handle<PhoneMicTray>,
    }

    pub fn spawn(data: Arc<Mutex<AppData>>) -> Handle {
        let tray = PhoneMicTray { data };
        let service = ksni::TrayService::new(tray);
        let handle = service.handle();
        service.spawn();
        Handle { inner: handle }
    }

    impl Handle {
        pub fn update(&self) {
            self.inner.update(|_| {});
        }
    }
}

// ---------------------------------------------------------------------------
// Windows: tray-item fallback
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
mod tray {
    use super::*;
    use tray_item::{IconSource, TrayItem};

    pub struct AppData {
        pub active: bool,
        pub tx: mpsc::Sender<TrayMessage>,
    }

    fn icon_for(active: bool) -> IconSource {
        if active { IconSource::Resource("media-record") } else { IconSource::Resource("phone-mic") }
    }

    fn build_tray_item(tx: mpsc::Sender<TrayMessage>, active: bool) -> TrayItem {
        let mut tray = TrayItem::new("Phone Mic", icon_for(active)).unwrap();
        tray.add_label("Phone Mic").unwrap();

        let label = if active { "Deactivate Phone Mic" } else { "Activate Phone Mic" };

        let toggle_tx = tx.clone();
        tray.add_menu_item(label, move || {
            let _ = toggle_tx.send(TrayMessage::Toggle);
        })
        .unwrap();

        tray.inner_mut().add_separator().unwrap();

        let quit_tx = tx;
        tray.add_menu_item("Quit", move || {
            let _ = quit_tx.send(TrayMessage::Quit);
        })
        .unwrap();

        tray
    }

    pub struct Handle {
        tray: Mutex<Option<TrayItem>>,
        tx: mpsc::Sender<TrayMessage>,
    }

    pub fn spawn(data: Arc<Mutex<AppData>>) -> Handle {
        let tx = data.lock().unwrap().tx.clone();
        let tray_item = build_tray_item(tx.clone(), false);
        Handle { tray: Mutex::new(Some(tray_item)), tx }
    }

    impl Handle {
        pub fn update(&self) {
            let has_tray = self.tray.lock().unwrap().is_some();
            let new = build_tray_item(self.tx.clone(), has_tray);
            *self.tray.lock().unwrap() = Some(new);
        }
    }
}

// ---------------------------------------------------------------------------

fn main() {
    let instance = single_instance::SingleInstance::new("phone-mic").unwrap();
    if !instance.is_single() {
        eprintln!("Another instance of phone-mic is already running");
        std::process::exit(1);
    }

    prime_adb();

    let (tx, rx) = mpsc::channel();
    let data = Arc::new(Mutex::new(tray::AppData {
        active: false,
        tx,
    }));

    let handle = tray::spawn(data.clone());
    let mut state = PhoneMicState::new();

    loop {
        match rx.recv() {
            Ok(TrayMessage::Toggle) => {
                if state.is_active() {
                    state.stop();
                    data.lock().unwrap().active = false;
                    handle.update();
                } else if let Err(e) = state.start() {
                    eprintln!("phone-mic error: {}", e);
                } else {
                    data.lock().unwrap().active = true;
                    handle.update();
                }
            }
            Ok(TrayMessage::Quit) => {
                state.stop();
                std::process::exit(0);
            }
            Err(_) => break,
        }
    }
}
