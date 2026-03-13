# Phonix

**Local voice dictation for Windows and macOS.** Hold a key, speak, release. Text appears in any window.

Phonix captures audio via your microphone, sends it to a Whisper-compatible API for transcription, optionally cleans up the text with an LLM, and pastes the result directly into whatever app has focus. Terminals, browsers, email, IDEs, Slack -- it works everywhere.

## Download & Install

### Windows

**Installer:** Download `PhonixSetup-x.x.x.exe` from the [Releases](../../releases/latest) page. Includes Start Menu shortcut, optional desktop shortcut, and uninstaller.

**Portable:** Download `phonix.exe` from the same page if you prefer a standalone binary with no installation.

### macOS (Alpha)

Download `Phonix-x.x.x-macos.dmg` from the [Releases](../../releases/latest) page.

**The app is not notarized**, so macOS Gatekeeper will block it. To install:

1. Open the DMG
2. Open **Terminal** and run:
   ```bash
   bash /Volumes/Phonix/install.sh
   ```
   This strips the quarantine flag and copies Phonix to /Applications.

   Or do it manually:
   ```bash
   cp -R /Volumes/Phonix/Phonix.app /Applications/
   xattr -cr /Applications/Phonix.app
   ```

On first launch, macOS will ask for two permissions:
- **Microphone** -- required for voice capture
- **Accessibility** (System Settings > Privacy & Security > Accessibility) -- required for hotkey detection and pasting text into other apps

**Important:** macOS support is alpha. It may have bugs. Please report issues on the [Issues](../../issues) page.

Phonix checks for updates automatically on launch and shows a notification banner if a newer version is available.

## Features

- **Push-to-talk dictation** -- hold a configurable hotkey, speak, release. Text is typed into the active window automatically.
- **Works in any app** -- uses Unicode keystroke injection (not clipboard), so it works in terminals, browser fields, IDEs, and desktop apps alike.
- **Pre-roll buffer** -- the microphone stays open with a rolling 0.8-second buffer so your first syllable is never clipped.
- **LLM cleanup** -- optionally run transcriptions through a local or cloud LLM to remove filler words, fix grammar, and clean up punctuation.
- **Multiple Whisper providers** -- Groq (free, fast), OpenAI, or a local whisper.cpp / faster-whisper server.
- **Long Dictate mode** -- click Start/Stop to record hands-free. Multiple recordings accumulate into a single text block. Copy when done.
- **History panel** -- every transcription is saved. Copy any entry with one click.
- **System tray** -- runs quietly in the background. Click the tray icon to open the UI.
- **Native overlay** -- a small always-on-top pill shows recording/transcribing/cleaning status near the top-right corner of your screen.
- **Auto-update** -- checks GitHub for new releases on startup and prompts you to download.
- **Dark theme UI** -- clean, modern interface built with egui.

## Quick Start

1. Download and run the installer (or portable exe)
2. Open Settings and paste your **Groq API key** (free at [groq.com](https://groq.com))
3. Hold **Right Alt**, speak, release
4. Text appears in whatever window was active

That's it. For LLM cleanup (optional), install [LM Studio](https://lmstudio.ai), load any small chat model, and start the local server.

## Configuration

All settings are in the UI (Settings tab) and saved to:
```
%APPDATA%\phonix\Phonix\config\config.toml
```

### Recording

| Setting | Default | Description |
|---------|---------|-------------|
| Record key | `RightAlt` (Windows) / `F13` (macOS) | Hold to record. Single keys or combos (e.g. `LeftCtrl+LeftShift`). Use "Record a combo" in Settings to capture multi-key combos. |
| Auto-paste | `true` | Type text into the active window after transcription |
| Sound effect | `true` | Play a tone on record start/stop |
| Close to tray | `true` | Hide to system tray instead of quitting when the window is closed |

**Note:** Changing the record key requires an app restart.

### Whisper (speech-to-text)

| Provider | Endpoint | Notes |
|----------|----------|-------|
| **Groq** (default) | `api.groq.com` | Free tier, fast. Requires API key. |
| **OpenAI** | `api.openai.com` | Paid. Requires API key. |
| **Local** | `localhost:8080` | Run your own whisper.cpp or faster-whisper server. No key needed. |

Advanced: override the URL or model name in Settings if you use a custom endpoint.

### Cleanup LLM (text polishing)

| Provider | Endpoint | Notes |
|----------|----------|-------|
| **Local** (default) | `localhost:1234` | LM Studio or any OpenAI-compatible server. No key needed. |
| **Groq** | `api.groq.com` | Reuses Whisper API key if both are Groq. |
| **OpenAI** | `api.openai.com` | Reuses Whisper API key if both are OpenAI. |

Cleanup removes filler words ("um", "uh", "like"), fixes punctuation, and polishes grammar while preserving your meaning. It can be disabled entirely in Settings.

## Building from Source

Requires [Rust](https://rustup.rs/) (2021 edition).

```bash
git clone https://github.com/mrchevyceleb/phonix.git
cd phonix
cargo build --release
```

The binary is at `target/release/phonix.exe` (Windows) or `target/release/phonix` (macOS).

### Local Whisper Server (optional)

If you want fully offline transcription, Phonix includes a Python wrapper for faster-whisper:

```bash
cd whisper-server
pip install -r requirements.txt
python server.py
```

Set the Whisper provider to "Local" in Settings. Dependencies are auto-installed on first launch if you select the Local provider from the app.

### Building the Installer

Requires [Inno Setup 6](https://jrsoftware.org/isinfo.php). After building the release binary:

```bash
iscc installer/phonix.iss
```

The installer is output to `target/installer/`.

## Architecture

Multi-threaded event-driven design:

```
Hotkey press -> capture foreground window -> start recording (with pre-roll)
Hotkey release -> stop recording -> encode WAV -> Whisper API -> raw text
-> (optional) LLM cleanup -> auto-paste into original window -> save to history
```

- **Main thread** -- egui UI loop
- **Hotkey thread** -- polls key state every 20ms (`GetAsyncKeyState` on Windows, `CGEventSourceKeyState` on macOS)
- **Pipeline thread** -- orchestrates recording, transcription, cleanup, paste
- **Overlay thread** -- native always-on-top status pill (GDI on Windows, NSWindow + CALayer on macOS)
- **Tokio runtime** -- async HTTP for Whisper and LLM APIs

Threads communicate via bounded crossbeam channels. The pipeline never blocks the UI.

## Tech Stack

- **Rust** with egui/eframe for the UI
- **CPAL** for cross-platform audio capture
- **tokio + reqwest** for async HTTP
- **crossbeam-channel** for inter-thread communication
- **windows crate** for native Windows APIs (hotkeys, SendInput, GDI overlay)
- **cocoa/objc/core-graphics** for native macOS APIs (hotkeys, CGEvent paste, NSWindow overlay)
- **serde + TOML** for configuration
- **Inno Setup** for Windows installer

## Platform Support

| Platform | Status |
|----------|--------|
| Windows 10/11 | Fully supported |
| macOS 12+ | Alpha (may have severe bugs) |
| Linux | Planned |

## License

[MIT](LICENSE)
