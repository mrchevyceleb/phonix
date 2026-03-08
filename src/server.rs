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
    pub fn kill_stale() {
        #[cfg(windows)]
        {
            // Find python processes listening on port 8080
            let _ = Command::new("cmd")
                .args(["/C", "for /f \"tokens=5\" %a in ('netstat -ano ^| findstr :8080 ^| findstr LISTENING') do taskkill /F /T /PID %a"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("sh")
                .args(["-c", "lsof -ti :8080 | xargs kill -9 2>/dev/null"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    /// Spawn the whisper server. Blocks briefly to install deps, then returns.
    /// Server readiness is checked separately via `wait_until_ready`.
    pub fn start(&mut self, server_py: &PathBuf) -> Result<(), String> {
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
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn whisper server: {e}"))?;

        self.child = Some(child);
        Ok(())
    }

    /// Poll localhost:8080 until the server accepts connections.
    /// Returns Ok after the server is up, Err if it times out.
    pub fn wait_until_ready(&self, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if is_server_ready() {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err("Whisper server did not start within 60s".to_string());
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

/// Send a real HTTP GET /health and return true if we get any response back.
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
    let mut buf = [0u8; 32];
    matches!(stream.read(&mut buf), Ok(n) if n > 0)
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
