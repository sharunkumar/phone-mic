# phone-mic

Use your phone as a microphone via scrcpy. System tray app written in Rust.

## Dependencies

- `scrcpy` — streams audio from your phone
- `pulseaudio-utils` — provides `parec` and `pactl`
- `adb` — Android Debug Bridge

## Usage

```
cargo run
```

Left-click the tray icon to toggle. Right-click for Quit.

## How it works

1. Creates a PulseAudio pipe source via `pactl load-module module-pipe-source`
2. Runs `parec` to create a recording stream from that source
3. Runs `scrcpy --audio-source=mic` to capture phone mic audio and pipe it in
4. The audio source appears in PulseAudio as "Scrcpy" — use it as any other input
