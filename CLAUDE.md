# Phonix

Local voice dictation for Windows. Hold a key, speak, release. Text appears in any window.

## Tech Stack

- **Language**: Rust 2021 edition
- **UI**: egui 0.28 / eframe (immediate-mode GUI)
- **Audio**: CPAL 0.15 (cross-platform capture)
- **Async**: tokio + reqwest 0.12
- **Concurrency**: crossbeam-channel for event passing between threads
- **Windows APIs**: windows 0.58 (hotkeys, SendInput paste, GDI overlay)
- **Whisper**: OpenAI-compatible API (Groq, OpenAI, or local faster-whisper)
- **LLM Cleanup**: LM Studio local server
- **Serialization**: serde + toml (config) + json (history)

## Project Structure

```
src/
  main.rs       # Entry point, runtime orchestration, tray icon, window lifecycle
  app.rs        # egui UI: History, Long Dictate, Settings tabs
  audio.rs      # CPAL mic capture with 0.8s pre-roll ring buffer
  cleanup.rs    # LLM cleanup via LM Studio, think-tag stripping
  config.rs     # Config struct, TOML persistence, WhisperProvider enum
  hotkey.rs     # GetAsyncKeyState polling (20ms), HWND capture
  overlay.rs    # Native GDI "REC" pill overlay (always-on-top)
  paste.rs      # Unicode SendInput keystroke injection
  server.rs     # Local whisper-server lifecycle (spawn, health-check, cleanup)
  sound.rs      # MessageBeep sound effects
  store.rs      # History entries (UUID, text, raw, timestamp), JSON persistence
  whisper.rs    # WAV encoding + multipart POST to Whisper endpoint
whisper-server/
  server.py     # Flask wrapper around faster-whisper (GPU/CPU auto-detect)
  requirements.txt
  start.bat
```

## Architecture

Multi-threaded event-driven design with bounded crossbeam channels:

1. **Main thread**: egui UI loop
2. **Hotkey thread**: polls GetAsyncKeyState every 20ms, emits RecordStart/RecordStop
3. **Pipeline thread**: orchestrates recording, transcription, cleanup, paste
4. **Overlay thread**: native GDI window with message pump
5. **Tokio runtime**: async HTTP requests (Whisper API, LM Studio)

### Data Flow

```
Hotkey press -> capture foreground HWND -> start recording (pre-roll seeded)
Hotkey release -> stop recording -> encode WAV -> Whisper API -> raw text
-> (optional) LLM cleanup -> auto-paste into original window -> save to history
```

### Key Patterns

- **Pre-roll buffer**: Mic stays open at startup, ring buffer holds last 0.8s. Prevents first-syllable clipping.
- **Channel events**: Pipeline threads communicate with UI via bounded crossbeam channels. No UI blocking.
- **Config reload**: Config re-read from disk on each transcription. Settings take effect immediately (except record key, which requires restart).
- **Graceful fallback**: LLM failure returns raw text. Paste failure logs but continues. Server failure shows error without crashing.
- **Think-tag stripping**: Removes `<think>...</think>` from reasoning models (DeepSeek). Falls back to raw if output is empty or >3x input length.

## Build and Run

```bash
cargo build --release
./target/release/phonix.exe
```

## Config Locations

- **Config**: `%APPDATA%/phonix/Phonix/config/config.toml`
- **History**: `%APPDATA%/phonix/Phonix/history.json`

## Conventions

- Error handling: `anyhow::Result` throughout
- Naming: snake_case for functions/variables, PascalCase for types/enums
- Module organization: one file per concern, flat structure under src/
- Windows-only: heavy use of `windows` crate for system integration
- No unsafe unless wrapping Windows API calls
- Release profile: opt-level 3, LTO, single codegen unit, stripped symbols

## Gotchas

- Record key change requires app restart (hardcoded into hotkey thread at spawn)
- Pre-roll samples are always included in Whisper input (0.8s prepended)
- Minimum speech duration: >0.5s excluding pre-roll (prevents transcribing silence)
- Paste uses Unicode SendInput (character-by-character), not clipboard
- Long Dictate mode suppresses auto-paste (text accumulates in UI instead)
- Python whisper-server dependencies are auto-installed via pip on first launch
- Tray icon color: blue when idle, red when recording
