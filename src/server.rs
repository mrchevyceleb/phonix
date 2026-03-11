use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
// CREATE_NO_WINDOW — prevents Python from opening a console window
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct WhisperServer {
    child: Option<Child>,
}

impl WhisperServer {
    pub fn new() -> Self {
        Self { child: None }
    }

    /// Kill any leftover whisper-server processes from previous runs.
    /// Prevents zombie Python processes from piling up if the app crashed.
    ///
    /// Two-phase approach:
    ///   1. Kill ALL python processes whose command line contains whisper-server/server.py.
    ///      This catches orphans that crashed during model loading (before binding port 8080)
    ///      and processes left behind when Phonix was force-closed.
    ///   2. Kill anything still listening on port 8080 as a fallback.
    pub fn kill_stale() {
        #[cfg(windows)]
        {
            // Phase 1: Kill by command line match (catches everything, including
            // processes that never bound port 8080 but still hold CUDA contexts).
            // Filter on Name='python.exe' to avoid self-matching the PowerShell
            // process whose own command line contains the query text.
            let _ = Command::new("powershell")
                .args([
                    "-NoProfile", "-Command",
                    "Get-CimInstance Win32_Process -Filter \"Name='python.exe' AND CommandLine like '%whisper-server%server.py%'\" | ForEach-Object { taskkill /F /T /PID $_.ProcessId 2>$null }",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status();

            // Phase 2: Also kill anything listening on port 8080.
            let _ = Command::new("cmd")
                .args(["/C", "for /f \"tokens=5\" %a in ('netstat -ano ^| findstr :8080 ^| findstr LISTENING') do taskkill /F /T /PID %a >nul 2>&1"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
        #[cfg(target_os = "macos")]
        {
            // Phase 1: Kill by command line match
            let _ = Command::new("sh")
                .args(["-c", "pkill -9 -f 'whisper-server.*server\\.py' 2>/dev/null"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            // Phase 2: Kill anything on port 8080
            let _ = Command::new("sh")
                .args(["-c", "lsof -ti :8080 | xargs kill -9 2>/dev/null"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    /// Spawn the whisper server. Blocks briefly to install deps, then returns.
    /// Server readiness is checked separately via `wait_until_ready`.
    pub fn start(&mut self, server_py: &PathBuf, model_arg: Option<&str>) -> Result<(), String> {
        let (exe, pre_args) = find_python()
            .ok_or_else(|| "Python not found. Install Python 3.x from python.org.".to_string())?;

        // Auto-install Flask + faster-whisper if needed
        let req = server_py.parent().unwrap().join("requirements.txt");
        if req.exists() {
            let mut cmd = Command::new(&exe);
            cmd.args(&pre_args);
            cmd.args(["-m", "pip", "install", "-r"]);
            cmd.arg(&req);
            cmd.arg("--quiet");
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            #[cfg(windows)]
            cmd.creation_flags(CREATE_NO_WINDOW);
            let _ = cmd.status(); // best-effort, ignore errors
        }

        let mut cmd = Command::new(&exe);
        cmd.args(&pre_args);
        cmd.arg(server_py);
        if let Some(model) = model_arg {
            cmd.args(["--model", model]);
        }
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn whisper server: {e}"))?;

        self.child = Some(child);
        Ok(())
    }

    /// Check if the server process exited early. Returns the stderr output if it did.
    pub fn check_early_exit(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stderr_output = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    use std::io::Read;
                    let _ = stderr.read_to_string(&mut stderr_output);
                }
                if stderr_output.is_empty() {
                    Some(format!("Server exited with {status}"))
                } else {
                    stderr_output.truncate(1000);
                    Some(format!("Server crashed: {stderr_output}"))
                }
            }
            _ => None,
        }
    }

    /// Poll localhost:8080 until the server accepts connections.
    /// Returns Ok after the server is up, Err if it times out.
    pub fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut port_conflict_detected = false;
        loop {
            if let Some(err) = self.check_early_exit() {
                return Err(err);
            }
            if is_server_ready() {
                return Ok(());
            }
            // If something responds on 8080 but it's not our server, flag it
            if !port_conflict_detected && is_port_occupied() {
                port_conflict_detected = true;
                eprintln!(
                    "[phonix/server] WARNING: Port 8080 is occupied by another application. \
                     Whisper server cannot bind. Close the conflicting app and restart Phonix."
                );
            }
            if start.elapsed() > timeout {
                if port_conflict_detected {
                    return Err(
                        "Port 8080 is occupied by another application. Close it and restart Phonix.".to_string()
                    );
                }
                return Err(
                    "Whisper server did not start within 60s. Check that Python 3 and its dependencies (flask, faster-whisper) are installed.".to_string()
                );
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        let pid = child.id();

        // On Windows, kill the whole process tree (Python spawns sub-processes)
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(not(windows))]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
    }
}

/// Check if something is listening on port 8080 (even if it's not our server).
fn is_port_occupied() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let Ok(mut stream) = TcpStream::connect_timeout(
        &"127.0.0.1:8080".parse().unwrap(),
        Duration::from_millis(300),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
    let mut buf = [0u8; 1];
    matches!(stream.read(&mut buf), Ok(n) if n > 0)
}

/// Find whisper-server/server.py relative to the running executable.
pub fn find_server_py() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;

    // Release build: whisper-server/ sits next to phonix.exe
    let p = exe_dir.join("whisper-server").join("server.py");
    if p.exists() {
        return Some(p);
    }

    // macOS .app bundle: binary is at Phonix.app/Contents/MacOS/phonix
    // Resources are at Phonix.app/Contents/Resources/
    let p = exe_dir
        .parent()  // Contents/MacOS -> Contents
        .map(|d| d.join("Resources").join("whisper-server").join("server.py"));
    if let Some(p) = p {
        if p.exists() {
            return Some(p);
        }
    }

    // Dev build: exe is at target/debug/ or target/release/ — go up to workspace root
    let p = exe_dir
        .parent()?
        .parent()?
        .join("whisper-server")
        .join("server.py");
    if p.exists() {
        return Some(p);
    }

    None
}

/// Public wrapper for use by the health-poll thread in main.rs.
pub fn is_server_ready_public() -> bool {
    is_server_ready()
}

/// Send a real HTTP GET /health and return true only if the response body is "ok".
/// This prevents false positives when another service (e.g. a proxy) occupies the port.
/// Unlike bare TCP polling, this completes the HTTP transaction so single-threaded
/// Flask doesn't block waiting for a request that never arrives.
fn is_server_ready() -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let Ok(mut stream) = TcpStream::connect_timeout(
        &"127.0.0.1:8080".parse().unwrap(),
        Duration::from_millis(300),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
    let mut buf = [0u8; 512];
    let Ok(n) = stream.read(&mut buf) else {
        return false;
    };
    let response = String::from_utf8_lossy(&buf[..n]);
    // Check for HTTP 200 and body containing "ok"
    response.contains("200") && response.contains("ok")
}

/// Find a working Python executable. Tries py -3.13, py -3.12, py, python3, python.
fn find_python() -> Option<(String, Vec<String>)> {
    let candidates: &[(&str, &[&str])] = &[
        ("py", &["-3.13"]),
        ("py", &["-3.12"]),
        ("py", &["-3.11"]),
        ("py", &["-3.10"]),
        ("py", &[]),
        ("python3", &[]),
        ("python", &[]),
    ];

    for (exe, args) in candidates {
        let ok = Command::new(exe)
            .args(*args)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            return Some((
                exe.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
        }
    }
    None
}
