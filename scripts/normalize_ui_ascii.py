from pathlib import Path
import re

path = Path("src/app.rs")
text = path.read_text(encoding="utf-8")

replacements = {
    "Ready — hold key to dictate": "Ready - hold key to dictate",
    "Recording…": "Recording...",
    "Transcribing…": "Transcribing...",
    "speech → text": "speech -> text",
    "text → polished text": "text -> polished text",
    "Advanced — override URL / model": "Advanced - override URL / model",
    "Text accumulates here — copy when done.": "Text accumulates here - copy when done.",
    "✓ Copied": "Copied",
    "● Live": "Live",
}

for old, new in replacements.items():
    text = text.replace(old, new)

# Emoji/icon label normalizations
text = text.replace("🎙", "MIC")
text = text.replace("⏹  Stop", "Stop")
text = text.replace("MIC  Start", "Start")
text = text.replace("📋  Copy All", "Copy All")
text = text.replace("⚙  Recording", "Recording")
text = text.replace("🎤  Whisper  (speech -> text)", "Whisper (speech -> text)")
text = text.replace("✨  Cleanup  (text -> polished text)", "Cleanup (text -> polished text)")

# Safety: if any icon prefix remains before known labels, strip it
text = re.sub(r'"[^"\\n]*\s{2}Whisper\s+\(speech -> text\)"', '"Whisper (speech -> text)"', text)
text = re.sub(r'"[^"\\n]*\s{2}Cleanup\s+\(text -> polished text\)"', '"Cleanup (text -> polished text)"', text)
text = re.sub(r'"[^"\\n]*\s{2}Copy All"', '"Copy All"', text)
text = re.sub(r'"[^"\\n]*\s{2}Start"', '"Start"', text)
text = re.sub(r'"[^"\\n]*\s{2}Stop"', '"Stop"', text)
text = re.sub(r'"[^"\\n]*\s{2}Recording"', '"Recording"', text)

path.write_text(text, encoding="utf-8")
print("Normalized src/app.rs")
