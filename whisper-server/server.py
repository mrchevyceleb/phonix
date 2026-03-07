"""
Phonix local Whisper server
Wraps faster-whisper in an OpenAI-compatible /v1/audio/transcriptions endpoint.

Usage:
    py -3.13 server.py

Then set Phonix → Settings → Provider → Local
"""

import os
import sys
import tempfile

# Register CUDA 12.6 DLLs before importing faster-whisper
cuda_path = r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.6\bin"
if os.path.exists(cuda_path):
    os.add_dll_directory(cuda_path)

from faster_whisper import WhisperModel
from http.server import BaseHTTPRequestHandler, HTTPServer
import cgi
import json

# ── Config ────────────────────────────────────────────────────────────────────

MODEL_SIZE = "medium"   # tiny / base / small / medium / large-v3
DEVICE     = "cuda"     # cuda or cpu
COMPUTE    = "float16"  # float16 (GPU) or int8 (CPU)
HOST       = "0.0.0.0"
PORT       = 8080

# ── Load model once at startup ────────────────────────────────────────────────

print(f"[whisper-server] Loading {MODEL_SIZE} on {DEVICE}...")
model = WhisperModel(MODEL_SIZE, device=DEVICE, compute_type=COMPUTE)
print(f"[whisper-server] Ready on http://localhost:{PORT}")

# ── HTTP handler ──────────────────────────────────────────────────────────────

class Handler(BaseHTTPRequestHandler):

    def log_message(self, fmt, *args):
        print(f"[whisper-server] {fmt % args}")

    def do_POST(self):
        if self.path not in ("/v1/audio/transcriptions", "/audio/transcriptions"):
            self.send_error(404)
            return

        # Parse multipart form data
        content_type = self.headers.get("Content-Type", "")
        if "multipart/form-data" not in content_type:
            self.send_error(400, "Expected multipart/form-data")
            return

        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)

        # Write body to a temp file so cgi can parse it
        with tempfile.NamedTemporaryFile(delete=False, suffix=".bin") as tmp:
            tmp.write(body)
            tmp_path = tmp.name

        try:
            # Re-parse multipart from the temp file
            import email
            raw = (
                f"Content-Type: {content_type}\r\n"
                f"Content-Length: {length}\r\n\r\n"
            ).encode() + body

            msg = email.message_from_bytes(raw)
            audio_data = None
            for part in msg.walk():
                cd = part.get("Content-Disposition", "")
                if 'name="file"' in cd:
                    audio_data = part.get_payload(decode=True)
                    break

            if audio_data is None:
                self.send_error(400, "No file field in form data")
                return

            # Write audio to temp WAV
            with tempfile.NamedTemporaryFile(delete=False, suffix=".wav") as af:
                af.write(audio_data)
                audio_path = af.name

            try:
                segments, _ = model.transcribe(
                    audio_path,
                    language="en",
                    beam_size=5,
                    vad_filter=True,
                )
                text = " ".join(seg.text.strip() for seg in segments).strip()
            finally:
                os.unlink(audio_path)

        finally:
            os.unlink(tmp_path)

        # Phonix sends response_format=text, return plain text
        response = text.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def do_GET(self):
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")
        else:
            self.send_error(404)


if __name__ == "__main__":
    server = HTTPServer((HOST, PORT), Handler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[whisper-server] Stopped.")
