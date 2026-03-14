//! Shell execution service with timeout, output capture, and duration tracking.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tokio::process::Command;

/// Result of a shell command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

impl ExecResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    /// Combined stdout + stderr for display.
    pub fn combined(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// Options for command execution.
pub struct ExecOpts {
    pub timeout_sec: u32,
    pub elevated: bool,
}

impl Default for ExecOpts {
    fn default() -> Self {
        Self {
            timeout_sec: 30,
            elevated: false,
        }
    }
}

/// Execute a shell command and capture output.
pub async fn exec(program: &str, args: &[&str], opts: ExecOpts) -> Result<ExecResult> {
    let start = Instant::now();

    let mut cmd = if opts.elevated {
        let mut c = Command::new("sudo");
        c.arg(program);
        c.args(args);
        c
    } else {
        let mut c = Command::new(program);
        c.args(args);
        c
    };

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(opts.timeout_sec as u64),
        cmd.output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Command timed out after {}s", opts.timeout_sec))??;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(ExecResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
        duration_ms,
    })
}

/// Execute a shell command string via /bin/sh -c.
pub async fn exec_shell(command: &str, timeout_sec: u32) -> Result<ExecResult> {
    let shell = if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "/bin/sh"
    };
    let flag = if cfg!(target_os = "windows") {
        "/C"
    } else {
        "-c"
    };
    exec(shell, &[flag, command], ExecOpts { timeout_sec, elevated: false }).await
}
