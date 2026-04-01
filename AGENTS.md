# AGENTS.md

## Cursor Cloud specific instructions

### Overview

**My Translator** is a Tauri 2 desktop app (Rust backend + vanilla JS/HTML/CSS frontend) for real-time speech translation. It captures system audio or microphone, sends it to Soniox API for STT + translation, and displays results in an overlay window with optional TTS narration.

### Platform constraint

The app officially targets **macOS** and **Windows** only. The Rust backend uses platform-specific audio capture: `screencapturekit` (macOS) and `windows` WASAPI (Windows). A Linux stub (`src-tauri/src/audio/linux_stub.rs`) has been added so the project compiles and runs on Linux, but system audio capture will return an error. Microphone capture via `cpal` works on Linux.

### Development commands

| Task | Command |
|------|---------|
| Install JS deps | `npm install` |
| Run dev mode | `npx tauri dev` |
| Build release | `npx tauri build` |
| Cargo check | `cargo check --manifest-path src-tauri/Cargo.toml` |
| Clippy lint | `cargo clippy --manifest-path src-tauri/Cargo.toml` |

### System dependencies (Linux)

These must be installed for Tauri to compile on Linux:

```
libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev libssl-dev pkg-config
```

### Key gotchas

- **Rust toolchain**: Requires Rust stable >= 1.85+ (the `time-core` crate uses `edition2024`). Run `rustup update stable && rustup default stable` if the pre-installed version is too old.
- **build.rs**: Contains a macOS-specific linker flag (`-Wl,-rpath,/usr/lib/swift`) guarded by `#[cfg(target_os = "macos")]`. Without this guard, linking fails on Linux.
- **First cargo build is slow**: ~600 crates, takes ~60-90 seconds on first compilation.
- **No bundler for frontend**: The frontend is plain HTML/CSS/JS served directly from `src/` — no build step required for frontend assets.
- **API keys**: The app requires a Soniox API key to perform translation. Without it, the app launches and the UI works, but pressing Play shows a validation error. This is expected behavior in development.
- **libEGL warnings**: When running on a headless Linux VM (Xvfb), you'll see `libEGL warning: DRI3 error` messages. These are harmless and the app renders correctly via software rendering.
