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

def _add_dll_dir(path):
    # ctranslate2 loads cuBLAS/cuDNN lazily via LoadLibrary, which searches PATH,
    # so register both ways.
    os.add_dll_directory(path)
    os.environ["PATH"] = path + os.pathsep + os.environ.get("PATH", "")

def _register_cuda():
    """Returns True if CUDA 12.x DLLs were found and registered."""
    if sys.platform != "win32":
        return True
    # pip-installed runtime (nvidia-cublas-cu12, nvidia-cudnn-cu12): site-packages/nvidia/*/bin
    import site
    pip_bins = []
    for sp in site.getsitepackages() + [site.getusersitepackages()]:
        nv = os.path.join(sp, "nvidia")
        if os.path.isdir(nv):
            pip_bins += [os.path.join(nv, d, "bin") for d in os.listdir(nv)
                         if os.path.isdir(os.path.join(nv, d, "bin"))]
    if any(os.path.exists(os.path.join(b, "cublas64_12.dll")) for b in pip_bins):
        for b in pip_bins:
            _add_dll_dir(b)
        print(f"[whisper-server] CUDA DLLs: pip nvidia packages ({len(pip_bins)} dirs)")
        return True
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
            _add_dll_dir(path)
            print(f"[whisper-server] CUDA DLLs: {path}")
            return True
    print("[whisper-server] No CUDA 12.x found — falling back to CPU")
    return False

cuda_available = _register_cuda()

# ── Imports (after CUDA registration) ────────────────────────────────────────

from faster_whisper import WhisperModel
from flask import Flask, request, Response

# ── Args ──────────────────────────────────────────────────────────────────────

parser = argparse.ArgumentParser(description="Phonix local Whisper server")
parser.add_argument("--model",  default="small",
                    choices=["tiny", "base", "small", "medium", "large-v2", "large-v3"],
                    help="Whisper model size (default: medium)")
parser.add_argument("--cpu",    action="store_true",
                    help="Force CPU inference (default: auto-detect GPU)")
parser.add_argument("--port",   type=int, default=8080,
                    help="Port to listen on (default: 8080)")
args = parser.parse_args()

device  = "cpu" if args.cpu or not cuda_available else "cuda"
compute = "int8" if device == "cpu" else "float16"

# ── Load model ────────────────────────────────────────────────────────────────

def load_model(model_name, device, compute_type):
    """Try to load a Whisper model with progressive fallbacks."""
    # 1. Try requested device
    print(f"[whisper-server] Loading {model_name} on {device} ({compute_type})...")
    try:
        return WhisperModel(model_name, device=device, compute_type=compute_type)
    except Exception as e:
        print(f"[whisper-server] Failed on {device}: {e}")

    # 2. Try CPU if we weren't already
    if device != "cpu":
        print(f"[whisper-server] Retrying {model_name} on CPU...")
        try:
            return WhisperModel(model_name, device="cpu", compute_type="int8")
        except Exception as e:
            print(f"[whisper-server] CPU load failed: {e}")

    # 3. Try progressively smaller models as last resort
    all_sizes = ["medium", "small", "base", "tiny"]
    try_from = all_sizes.index(model_name) + 1 if model_name in all_sizes else 0
    for fallback in all_sizes[try_from:]:
        print(f"[whisper-server] Trying smaller model: {fallback} on CPU...")
        try:
            return WhisperModel(fallback, device="cpu", compute_type="int8")
        except Exception as e:
            print(f"[whisper-server] {fallback} failed: {e}")

    return None

model = load_model(args.model, device, compute)
if model is None:
    print("[whisper-server] FATAL: Could not load any Whisper model.")
    print("[whisper-server] Try: pip install --force-reinstall faster-whisper ctranslate2")
    sys.exit(1)

print(f"[whisper-server] Ready — http://localhost:{args.port}")

# ── Flask app ─────────────────────────────────────────────────────────────────

app = Flask(__name__)

@app.route("/health")
def health():
    return "ok"

def load_audio(path):
    """Decode Phonix's 16-bit PCM WAV to a 16 kHz float32 array.
    Bypasses faster-whisper's PyAV decoder, which breaks whenever PyAV ships an
    incompatible release (e.g. av 16+ dropped the metadata_errors kwarg).
    Falls back to the path (PyAV decode) for anything that isn't plain PCM WAV."""
    import wave
    import numpy as np
    try:
        with wave.open(path, "rb") as w:
            if w.getsampwidth() != 2:
                return path
            rate, channels = w.getframerate(), w.getnchannels()
            pcm = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16)
    except wave.Error:
        return path
    audio = pcm.astype(np.float32) / 32768.0
    if channels > 1:
        audio = audio.reshape(-1, channels).mean(axis=1)
    if rate != 16000 and len(audio):
        n_out = int(len(audio) * 16000 / rate)
        audio = np.interp(
            np.linspace(0, len(audio) - 1, n_out), np.arange(len(audio)), audio
        ).astype(np.float32)
    return audio

def run_transcribe(audio_in):
    segments, _ = model.transcribe(
        audio_in,
        language="en",
        beam_size=5,
        vad_filter=True,
        vad_parameters=dict(
            speech_pad_ms=800,
        ),
    )
    return " ".join(seg.text.strip() for seg in segments).strip()

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

    global model
    try:
        audio_in = load_audio(tmp_path)
        try:
            text = run_transcribe(audio_in)
        except RuntimeError as e:
            # CUDA libs can load at model init but fail on first use (missing
            # cuBLAS/cuDNN). Switch to CPU for the rest of the session and retry.
            if device == "cpu" or getattr(model, "_phonix_cpu", False):
                raise
            print(f"[whisper-server] GPU inference failed ({e}) — switching to CPU")
            model = WhisperModel(args.model, device="cpu", compute_type="int8")
            model._phonix_cpu = True
            text = run_transcribe(audio_in)
    finally:
        os.unlink(tmp_path)

    # Phonix sends response_format=text — return plain text
    return Response(text, mimetype="text/plain")


if __name__ == "__main__":
    # Single-threaded: ctranslate2 allocates a CUDA context per thread.
    # With threaded=True, each Flask request thread creates a new context
    # that leaks VRAM even after the thread exits. Single-threaded mode
    # reuses one context for all requests, eliminating the leak.
    app.run(host="127.0.0.1", port=args.port, debug=False, threaded=False)
