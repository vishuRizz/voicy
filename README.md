# VoiceKey

> Hold a key. Speak. Text appears — processed entirely on your device.

VoiceKey is a lightweight macOS dictation utility built with **Tauri 2 · React · TypeScript · Rust · whisper.cpp**.  
Hold a configurable global shortcut, see a live provisional transcript, release to finalize and insert text into the app you were using.

---

## Running the app

### Start
```sh
cd /Users/vishupratap/Movies/voice-key
source ~/.cargo/env          # make cargo available
export PATH="/opt/homebrew/bin:$PATH"   # make cmake/brew tools available
npm run tauri -- dev
```

The first build takes ~60 s (compiling whisper.cpp). Subsequent runs take ~5–8 s.

### Open the window
VoiceKey lives in your **menu bar** (top-right of your screen).  
- **Menu bar icon** → click it → **Settings…** to open the main window  
- If the window doesn't appear, check if it's hidden behind other windows (Cmd+Tab)

### Grant permissions (first run only)
1. Press your hotkey (`Option+Space`) — macOS shows a microphone prompt → **Allow**
2. Click **Allow Accessibility** in the onboarding screen → enable VoiceKey in System Settings → Accessibility

### Dictate
1. Hold `Option+Space` (or your configured shortcut)
2. Speak — a live preview appears in the overlay
3. Release → text is inserted into whatever app you were using

### Stop / quit
```sh
# From the menu bar:  click icon → Quit VoiceKey

# Or from terminal (kills dev server + app):
pkill -f "target/debug/voicekey"; pkill -f "tauri dev"; pkill -f vite
```

---

## Features

| Feature | Status |
|---|---|
| Global hold-to-talk shortcut | ✅ Core |
| Live provisional transcript preview | ✅ Core |
| Local Whisper inference (no cloud) | ✅ Core |
| Text insertion via Accessibility API | 🔶 Stub → TODO |
| Clipboard-paste fallback | ✅ Core |
| Cancel recording (Esc or button) | ✅ Core |
| macOS permission onboarding | ✅ Core |
| Settings (shortcut · model · language) | ✅ Core |
| Windows support | Phase 4 |

---

## Prerequisites

| Tool | Version |
|---|---|
| Rust | ≥ 1.77 (`rustup install stable`) |
| Node.js | ≥ 18 |
| Xcode Command Line Tools | macOS |

Install Rust:
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Install Tauri CLI:
```sh
cargo install tauri-cli --version "^2"
```

---

## Getting started

```sh
# 1. Install JS dependencies
npm install

# 2. Download a Whisper model (base recommended for MVP)
./scripts/download_model.sh base

# 3. Run in dev mode
cargo tauri dev

# 4. Build a release .app
cargo tauri build
```

---

## Project structure

```
voicekey/
├── src/                        React + TypeScript UI
│   ├── app/App.tsx             Main shell (onboarding ↔ settings flow)
│   ├── components/
│   │   ├── ListeningOverlay    Always-on-top recording indicator
│   │   ├── Settings            Settings panel
│   │   └── PermissionOnboarding
│   ├── hooks/
│   │   ├── useSession          Subscribes to Rust session events
│   │   └── useSettings         Loads/saves settings via Tauri store
│   ├── lib/tauri.ts            Typed Tauri command + event wrappers
│   └── types/index.ts          Shared TypeScript types
├── src-tauri/
│   ├── src/
│   │   ├── main.rs             Binary entry point
│   │   ├── lib.rs              Tauri builder, plugins, managed state
│   │   ├── app_state.rs        Recording state machine + session IDs
│   │   ├── audio.rs            cpal microphone capture, 16 kHz mono PCM
│   │   ├── transcription.rs    Rolling-window preview + final inference
│   │   ├── insertion.rs        Text insertion orchestrator
│   │   ├── hotkey.rs           Global shortcut registration + key events
│   │   ├── settings.rs         User preferences + Tauri store persistence
│   │   ├── errors.rs           Structured error codes (TRD §22)
│   │   └── platform/
│   │       ├── macos.rs        macOS-specific insertion + permissions
│   │       └── windows.rs      Windows adapter stubs (Phase 4)
│   ├── capabilities/default.json
│   └── tauri.conf.json
├── models/                     Whisper ggml model files (gitignored)
├── scripts/download_model.sh   Model download helper
└── entitlements.plist          macOS entitlements
```

---

## Architecture

```
React UI (Webview)
    │  Tauri commands / events
    ▼
Rust Core
  ├── AppState      (state machine: IDLE→LISTENING→FINALIZING→INSERTING→IDLE)
  ├── HotkeyManager (global shortcut via tauri-plugin-global-shortcut)
  ├── AudioManager  (cpal, 16 kHz mono PCM, bounded in-memory buffer)
  ├── TranscriptionCoordinator
  │     ├── Rolling-window preview (every ~1 s, last ~5 s of audio)
  │     └── Final full-utterance pass (on key-up)
  └── InsertionManager
        ├── macOS: AX direct insertion (TODO) → clipboard fallback
        └── Windows: SendInput (Phase 4) → clipboard fallback
```

**Key design decisions:**
- Every session has a UUID.  Stale inference results from an old session are silently dropped.
- Audio stays in RAM only.  No disk writes, no cloud calls.
- The overlay window is `acceptFirstMouse: false` — it never steals focus.
- The Rust core is authoritative for state; UI events are presentation only.

---

## Permissions required (macOS)

| Permission | Why |
|---|---|
| Microphone | Capture speech for local transcription |
| Accessibility | Insert text into the focused application |
| Input Monitoring | Detect global key-up events outside VoiceKey |

---

## TODO before shipping

- [ ] Replace `run_whisper` stub with real `whisper-rs` FFI calls
- [ ] Implement `insert_text_direct` using `core-graphics` CGEvent keyboard injection
- [ ] Add tray icon with Settings / Quit menu
- [ ] Add model download progress UI (`install_model` command)
- [ ] Set numeric performance targets after benchmarking on reference hardware
- [ ] Code-sign and notarize the macOS build
- [ ] Privacy + permission review (TRD §19)

---

## Privacy

- **All inference is local** — no audio or text is sent to any server.
- Audio is kept in memory for the session duration, then discarded.
- No transcript content is written to logs.
- See `src-tauri/src/errors.rs` for the diagnostic-safe error policy.

---

## License

MIT — see `LICENSE`.


## my personal run
cd /Users/vishupratap/Movies/voice-key
source ~/.cargo/env && export PATH="/opt/homebrew/bin:$PATH"
npm run tauri -- dev


## export
source ~/.cargo/env
npm run tauri -- build --bundles dmg


## windows build
npm run tauri -- build