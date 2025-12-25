use anyhow::{Context, Result};
use nix::unistd::{fork, setsid, ForkResult};
use std::fs::{self, File};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::config::{get_config_dir, get_pid_file, get_socket_path, load_config, Config};
use crate::pid::{check_daemon, remove_pid_file, write_pid_file, PidInfo};

pub struct Daemon {
    #[allow(dead_code)]
    config: Config,
    socket_path: PathBuf,
    pid_path: PathBuf,
}

impl Daemon {
    pub fn new() -> Result<Self> {
        let config = load_config()?;
        let socket_path = get_socket_path(&config)?;
        let pid_path = get_pid_file(&config)?;

        if let Some(info) = check_daemon(&pid_path)? {
            anyhow::bail!(
                "Daemon already running (PID: {}, started: {})",
                info.pid,
                info.started_at
            );
        }

        Ok(Self {
            config,
            socket_path,
            pid_path,
        })
    }

    pub fn start(self) -> Result<()> {
        // Ensure config directory exists before forking
        if let Some(parent) = self.socket_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Fork to background
        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                println!("Daemon started (PID: {})", child);
                return Ok(());
            }
            Ok(ForkResult::Child) => {
                // Continue in child process
            }
            Err(e) => {
                anyhow::bail!("Fork failed: {}", e);
            }
        }

        // Child process: become session leader
        setsid().context("Failed to create new session")?;

        // Redirect stdin/stdout/stderr to /dev/null
        let dev_null = File::open("/dev/null")?;
        let null_fd = dev_null.as_raw_fd();
        unsafe {
            libc::dup2(null_fd, 0); // stdin
            libc::dup2(null_fd, 1); // stdout
            libc::dup2(null_fd, 2); // stderr
        }

        // Set up logging to file
        let log_path = get_config_dir().ok().map(|d| d.join("daemon.log"));
        if let Some(ref path) = log_path {
            if let Ok(file) = File::create(path) {
                let file_fd = file.as_raw_fd();
                unsafe {
                    libc::dup2(file_fd, 1); // stdout to log
                    libc::dup2(file_fd, 2); // stderr to log
                }
            }
        }

        // Build and run tokio runtime in daemon process
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.run_daemon())
    }

    async fn run_daemon(self) -> Result<()> {
        if self.socket_path.exists() {
            fs::remove_file(&self.socket_path)
                .with_context(|| format!("Failed to remove stale socket: {}", self.socket_path.display()))?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("Failed to bind socket: {}", self.socket_path.display()))?;

        tracing::info!("Daemon listening on {}", self.socket_path.display());

        let pid_info = PidInfo::new(process::id(), &self.socket_path);
        write_pid_file(&self.pid_path, &pid_info)?;

        let result = self.run_server(listener).await;
        self.cleanup();
        result
    }

    async fn run_server(&self, listener: UnixListener) -> Result<()> {
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream).await {
                                    tracing::error!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received shutdown signal");
                    break;
                }
            }
        }
        Ok(())
    }

    fn cleanup(&self) {
        if let Err(e) = remove_pid_file(&self.pid_path) {
            tracing::error!("Failed to remove PID file: {}", e);
        }
        if self.socket_path.exists() {
            if let Err(e) = fs::remove_file(&self.socket_path) {
                tracing::error!("Failed to remove socket file: {}", e);
            }
        }
    }
}

async fn handle_connection(stream: UnixStream) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }

        let request: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                let error_response = serde_json::json!({"error": format!("Invalid JSON: {}", e)});
                writer.write_all(error_response.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
                continue;
            }
        };

        let response = handle_request(&request).await;
        writer.write_all(response.to_string().as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }

    Ok(())
}

async fn handle_request(request: &serde_json::Value) -> serde_json::Value {
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");

    match method {
        "ping" => serde_json::json!({
            "result": "pong",
            "version": env!("CARGO_PKG_VERSION")
        }),
        "status" => serde_json::json!({
            "result": {
                "status": "running",
                "version": env!("CARGO_PKG_VERSION"),
                "pid": process::id()
            }
        }),
        _ => serde_json::json!({"error": format!("Unknown method: {}", method)}),
    }
}

pub async fn stop_daemon() -> Result<()> {
    let config = load_config()?;
    let pid_path = get_pid_file(&config)?;

    let info = match check_daemon(&pid_path)? {
        Some(info) => info,
        None => {
            println!("Daemon is not running");
            return Ok(());
        }
    };

    println!("Stopping daemon (PID: {})...", info.pid);
    crate::pid::send_sigterm(info.pid)?;

    for _ in 0..50 {
        if !crate::pid::is_process_running(info.pid) {
            println!("Daemon stopped");
            let _ = remove_pid_file(&pid_path);
            let socket_path = get_socket_path(&config)?;
            if socket_path.exists() {
                let _ = fs::remove_file(&socket_path);
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    anyhow::bail!("Daemon did not stop within 5 seconds")
}

pub fn daemon_status() -> Result<()> {
    let config = load_config()?;
    let pid_path = get_pid_file(&config)?;

    match check_daemon(&pid_path)? {
        Some(info) => {
            let running_version = &info.version;
            let cli_version = env!("CARGO_PKG_VERSION");

            println!("Daemon status: running");
            println!("  PID: {}", info.pid);
            println!("  Version: {}", running_version);
            println!("  Socket: {}", info.socket);
            println!("  Started: {}", info.started_at);

            if running_version != cli_version {
                println!();
                println!("Warning: Daemon version ({}) differs from CLI version ({})",
                         running_version, cli_version);
                println!("Consider running: memex daemon restart");
            }
        }
        None => {
            println!("Daemon status: not running");
        }
    }

    Ok(())
}
