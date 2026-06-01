use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

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

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(PhoneMicState::new()))
        .setup(|app| {
            let toggle = MenuItem::with_id(app, "toggle", "Activate Phone Mic", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &quit])?;

            let toggle = toggle.clone();

            TrayIconBuilder::new()
                .icon(tauri::include_image!("icons/32x32.png"))
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    let state = app.state::<Mutex<PhoneMicState>>();
                    let mut state = state.lock().unwrap();

                    match event.id().as_ref() {
                        "toggle" => {
                            if state.is_active() {
                                state.stop();
                                let _ = toggle.set_text("Activate Phone Mic");
                            } else if let Err(e) = state.start() {
                                eprintln!("phone-mic error: {}", e);
                            } else {
                                let _ = toggle.set_text("Deactivate Phone Mic");
                            }
                        }
                        "quit" => {
                            state.stop();
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
