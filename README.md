# Phonix

Local voice dictation that works in any text box — terminals, email, docs, browsers, everything.

Hold a key. Speak. Release. Text appears.

## How it works

1. Hold your record key (default: Right Alt)
2. Speak naturally
3. Release — audio goes to Whisper for transcription, then through an LLM for cleanup
4. Clean text is pasted into whatever window is active

All recordings are saved in the history panel so you can copy them manually if paste fails.

## Setup

### 1. Get a Whisper endpoint

**Option A — Groq (recommended for speed):** Free account at groq.com, grab an API key.

**Option B — Local whisper.cpp server:**
```bash
./server -m models/ggml-large-v3.bin --port 8080
# Then set whisper_url = "http://localhost:8080" in Phonix settings
```

**Option C — OpenAI API:** Use your existing key.

### 2. Set up LM Studio (for cleanup)

- Download LM Studio from lmstudio.ai
- Load any chat model (llama3, mistral, qwen, etc.)
- Start the local server (default: http://localhost:1234)
- In Phonix settings, set the model name to match what LM Studio shows

### 3. Build and run

```bash
# Requires Rust: rustup.rs
cargo build --release
./target/release/phonix.exe
```

## Configuration

Settings live in the UI (Settings tab) and are saved to:
`%APPDATA%\phonix\Phonix\config\config.toml`

| Setting | Default | Description |
|---------|---------|-------------|
| `record_key` | `RightAlt` | Key to hold while speaking |
| `auto_paste` | `true` | Paste into active window after transcription |
| `whisper_url` | Groq endpoint | Whisper-compatible API URL |
| `whisper_model` | `whisper-large-v3` | Model name |
| `cleanup_enabled` | `true` | Run LLM cleanup pass |
| `cleanup_url` | `http://localhost:1234/v1` | LM Studio endpoint |
| `cleanup_model` | `local-model` | Model loaded in LM Studio |

**Record key options:** `RightAlt`, `RightControl`, `LeftAlt`, `LeftControl`, `CapsLock`, `ScrollLock`, `F13`-`F16`

## UI

- **History** — every transcription saved, newest first. One-click copy on any entry.
- **Long Dictate** — toggle mode that accumulates text instead of pasting. Copy all when done.
- **Settings** — configure endpoints and keys without editing TOML.
- **System tray** — minimizes to tray, click to reopen.

## Open source

MIT license. PRs welcome.

Planned:
- [ ] macOS support
- [ ] Linux support
- [ ] Bundled whisper.cpp (no server needed)
- [ ] Custom cleanup prompts
- [ ] Per-app tone profiles (formal for email, casual for Slack)
