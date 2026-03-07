@echo off
echo [whisper-server] Installing dependencies...
py -m pip install -r "%~dp0requirements.txt" --quiet

echo [whisper-server] Starting...
py "%~dp0server.py" %*
pause
