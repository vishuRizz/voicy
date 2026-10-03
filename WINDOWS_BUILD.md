# 🪟 Building Voicy on Windows

This guide walks you through building Voicy from source on a Windows machine.
No Mac needed — everything runs natively on Windows 10 or 11.

---

## ✅ Prerequisites (one-time setup)

Install all of the following before building. Each step links to the official download.

---

### 1. Node.js (LTS)

> Required to run the frontend build tools (`npm`, `vite`).

**Download:** [https://nodejs.org/en/download](https://nodejs.org/en/download)

- Choose the **Windows Installer (.msi)** → LTS version
- Run the installer, keep all defaults
- Verify in a new terminal:
  ```powershell
  node --version   # should print v20.x or later
  npm --version
  ```

---

### 2. Rust (via rustup)

> Required to compile the Tauri backend (written in Rust).

**Download:** [https://rustup.rs](https://rustup.rs)

- Click **"Download rustup-init.exe"** and run it
- When prompted, choose option `1` (default install)
- **During install**, Rust will ask you to install Microsoft C++ Build Tools — say **Yes** (details in step 3)
- After install, close and reopen your terminal
- Verify:
  ```powershell
  rustc --version   # should print rustc 1.77 or later
  cargo --version
  ```

---

### 3. Microsoft C++ Build Tools (MSVC)

> Required by Rust on Windows to compile native code. Without this, nothing will compile.

**Download:** [https://visualstudio.microsoft.com/visual-cpp-build-tools/](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

- Click **"Download Build Tools"** and run the installer
- In the installer, check **"Desktop development with C++"**

  ![Workload selection](https://i.imgur.com/example.png)
  *(The checkbox is in the top-left of the workload grid)*

- Click **Install** (this downloads ~5–7 GB — let it finish)
- Restart your PC after it completes

> 💡 **If Rust already installed without MSVC:** Run `rustup component add rust-std` and then re-run the Build Tools installer.

---

### 4. WebView2 Runtime

> Tauri uses WebView2 (Microsoft Edge engine) to render the app UI.

**On Windows 10 (2004+) and Windows 11:** WebView2 is already installed — skip this step.

**On older Windows 10 or fresh installs:**

**Download:** [https://developer.microsoft.com/en-us/microsoft-edge/webview2/](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)

- Scroll down to **"Evergreen Bootstrapper"**
- Download and run `MicrosoftEdgeWebview2Setup.exe`

---

### 5. Git

> Needed to clone the repository.

**Download:** [https://git-scm.com/download/win](https://git-scm.com/download/win)

- Run the installer, keep all defaults
- Verify:
  ```powershell
  git --version
  ```

---

## 🔨 Building Voicy

Once all prerequisites are installed, open **PowerShell** or **Command Prompt** and run:

### Step 1 — Clone the repository

```powershell
git clone https://github.com/vishuRizz/voicy.git
cd voicy
```

### Step 2 — Install JavaScript dependencies

```powershell
npm install
```

> This installs all frontend packages. Takes ~1–2 minutes.

### Step 3 — Build the app

```powershell
npm run tauri -- build --bundles nsis
```

> **This takes 3–10 minutes** on first run — Rust compiles everything from scratch.
> Subsequent builds are much faster.

### Step 4 — Find your installer

After the build completes, the `.exe` installer will be at:

```
voicy\src-tauri\target\release\bundle\nsis\Voicy_0.1.0_x64-setup.exe
```

Double-click it to install Voicy. Done! 🎉

---

## ⚠️ Troubleshooting

### `error: linker 'link.exe' not found`
The Microsoft C++ Build Tools are not installed or not on your PATH.
→ Reinstall from [step 3](#3-microsoft-c-build-tools-msvc) and restart your PC.

### `npm: command not found`
Node.js is not installed or not on your PATH.
→ Reinstall from [step 1](#1-nodejs-lts) and open a **new** terminal window.

### `cargo: command not found`
Rust is not on your PATH.
→ Close and reopen your terminal after installing Rust. If that doesn't help, run:
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
```

### `WebView2 runtime not found` error at startup
→ Install WebView2 from [step 4](#4-webview2-runtime).

### Build takes forever / runs out of memory
Rust release builds are RAM-hungry.
- Close other apps during the build
- Minimum recommended: **8 GB RAM**
- If you have <8 GB, try the debug build instead:
  ```powershell
  npm run tauri -- build --debug --bundles nsis
  ```

### Microphone not detected
Go to **Settings → Privacy & Security → Microphone** and make sure
**"Let apps access your microphone"** is turned **On**, and Voicy is allowed.

---

## 🧐 What does the build produce?

| File | Description |
|---|---|
| `Voicy_0.1.0_x64-setup.exe` | NSIS installer — share this with others |
| `voicekey.exe` | Raw executable (no installer) |

The `.exe` installer handles:
- Installing the app to `%LocalAppData%\Programs\Voicy`
- Creating a Start Menu shortcut
- Adding an uninstaller to **Add/Remove Programs**

---

## 🤖 Want pre-built binaries?

If you don't want to build from source, a GitHub Actions workflow can auto-build
Windows binaries on every push and publish them to GitHub Releases.
Ask the developer to set this up — no Windows machine needed on your end.

---

*Built with [Tauri 2](https://tauri.app) + [Rust](https://www.rust-lang.org) + [React](https://react.dev)*
