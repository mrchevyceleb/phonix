"""
Phonix local Whisper server
Wraps faster-whisper in an OpenAI-compatible /v1/audio/transcriptions endpoint.

Requirements:
    pip install faster-whisper flask

Usage:
    py -3.13 server.py          # GPU (CUDA)
    py server.py --cpu          # CPU only
    py server.py --model small  # different model size
"""

import os
import sys
import argparse
import tempfile

# ── CUDA setup ────────────────────────────────────────────────────────────────
# ctranslate2 needs CUDA 12.x DLLs on PATH. We check common install locations
# and register whichever we find. Safe to skip if not on Windows or no CUDA.

def _register_cuda():
    if sys.platform != "win32":
        return
    cuda_candidates = [
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.6\bin",
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.5\bin",
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4\bin",
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.3\bin",
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.2\bin",
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.1\bin",
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.0\bin",
    ]
    for path in cuda_candidates:
        if os.path.exists(path):
            os.add_dll_directory(path)
            print(f"[whisper-server] CUDA DLLs: {path}")
            return
    print("[whisper-server] No CUDA 12.x found — falling back to CPU")

_register_cuda()

# ── Imports (after CUDA registration) ────────────────────────────────────────

from faster_whisper import WhisperModel
from flask import Flask, request, Response

# ── Args ──────────────────────────────────────────────────────────────────────

parser = argparse.ArgumentParser(description="Phonix local Whisper server")
parser.add_argument("--model",  default="medium",
                    choices=["tiny", "base", "small", "medium", "large-v2", "large-v3"],
                    help="Whisper model size (default: medium)")
parser.add_argument("--cpu",    action="store_true",
                    help="Force CPU inference (default: auto-detect GPU)")
parser.add_argument("--port",   type=int, default=8080,
                    help="Port to listen on (default: 8080)")
args = parser.parse_args()

device  = "cpu" if args.cpu else "cuda"
compute = "int8" if args.cpu else "float16"

# ── Load model ────────────────────────────────────────────────────────────────

print(f"[whisper-server] Loading {args.model} on {device}...")
try:
    model = WhisperModel(args.model, device=device, compute_type=compute)
except Exception as e:
    print(f"[whisper-server] GPU load failed ({e}), retrying on CPU...")
    model = WhisperModel(args.model, device="cpu", compute_type="int8")

print(f"[whisper-server] Ready — http://localhost:{args.port}")

# ── Flask app ─────────────────────────────────────────────────────────────────

app = Flask(__name__)

@app.route("/health")
def health():
    return "ok"

@app.route("/v1/audio/transcriptions", methods=["POST"])
@app.route("/audio/transcriptions", methods=["POST"])
def transcribe():
    if "file" not in request.files:
        return {"error": "missing 'file' field"}, 400

    audio = request.files["file"]

    # Save to a temp file — faster-whisper needs a path, not a stream
    suffix = os.path.splitext(audio.filename or ".wav")[1] or ".wav"
    with tempfile.NamedTemporaryFile(delete=False, suffix=suffix) as tmp:
        audio.save(tmp.name)
        tmp_path = tmp.name

    try:
        segments, _ = model.transcribe(
            tmp_path,
            language="en",
            beam_size=5,
            vad_filter=True,
        )
        text = " ".join(seg.text.strip() for seg in segments).strip()
    finally:
        os.unlink(tmp_path)

    # Phonix sends response_format=text — return plain text
    return Response(text, mimetype="text/plain")


if __name__ == "__main__":
    app.run(host="0.0.0.0", port=args.port, debug=False, threaded=True)
