# Phonix

**Local voice dictation for Windows.** Hold a key, speak, release. Text appears in any window.

Phonix captures audio via your microphone, sends it to a Whisper-compatible API for transcription, optionally cleans up the text with an LLM, and pastes the result directly into whatever app has focus. Terminals, browsers, email, IDEs, Slack -- it works everywhere.

## Features

- **Push-to-talk dictation** -- hold a configurable hotkey, speak, release. Text is typed into the active window automatically.
- **Works in any app** -- uses Unicode keystroke injection (not clipboard), so it works in terminals, browser fields, IDEs, and desktop apps alike.
- **Pre-roll buffer** -- the microphone stays open with a rolling 0.8-second buffer so your first syllable is never clipped.
- **LLM cleanup** -- optionally run transcriptions through a local or cloud LLM to remove filler words, fix grammar, and clean up punctuation.
- **Multiple Whisper providers** -- Groq (free, fast), OpenAI, or a local whisper.cpp / faster-whisper server.
- **Long Dictate mode** -- accumulate multiple recordings into a single block of text instead of auto-pasting each one.
- **History panel** -- every transcription is saved. Copy any entry with one click.
- **System tray** -- runs quietly in the background. Click the tray icon to open the UI.
- **Native overlay** -- a small always-on-top pill shows recording/transcribing/cleaning status near the top-right corner of your screen.
- **Dark theme UI** -- clean, modern interface built with egui.

## Download

Grab the latest `phonix.exe` from the [Releases](../../releases) page. No installer needed -- just run the exe.

## Quick Start

1. Download and run `phonix.exe`
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
| Record key | `RightAlt` | Hold to record. Options: `RightAlt`, `LeftAlt`, `RightControl`, `LeftControl`, `CapsLock`, `ScrollLock`, `F13`--`F16` |
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

The binary is at `target/release/phonix.exe`.

### Local Whisper Server (optional)

If you want fully offline transcription, Phonix includes a Python wrapper for faster-whisper:

```bash
cd whisper-server
pip install -r requirements.txt
python server.py
```

Set the Whisper provider to "Local" in Settings. Dependencies are auto-installed on first launch if you select the Local provider from the app.

## Architecture

Multi-threaded event-driven design:

```
Hotkey press -> capture foreground window -> start recording (with pre-roll)
Hotkey release -> stop recording -> encode WAV -> Whisper API -> raw text
-> (optional) LLM cleanup -> auto-paste into original window -> save to history
```

- **Main thread** -- egui UI loop
- **Hotkey thread** -- polls `GetAsyncKeyState` every 20ms
- **Pipeline thread** -- orchestrates recording, transcription, cleanup, paste
- **Overlay thread** -- native GDI always-on-top status pill
- **Tokio runtime** -- async HTTP for Whisper and LLM APIs

Threads communicate via bounded crossbeam channels. The pipeline never blocks the UI.

## Tech Stack

- **Rust** with egui/eframe for the UI
- **CPAL** for cross-platform audio capture
- **tokio + reqwest** for async HTTP
- **crossbeam-channel** for inter-thread communication
- **windows crate** for native Windows APIs (hotkeys, SendInput, GDI overlay)
- **serde + TOML** for configuration

## Platform Support

| Platform | Status |
|----------|--------|
| Windows 10/11 | Fully supported |
| macOS | Planned |
| Linux | Planned |

## License

[MIT](LICENSE)
