//! Plugin loader — loads external domain definitions.
//!
//! A plugin is a directory containing:
//!   spec.json     — domain spec (observations + actions)
//!   dispatch.json — vtable (target+verb → handler) + policy
//!   handler.py    — executable that handles observe/act via stdin/stdout JSON
//!   (or handler, handler.sh, etc.)
//!
//! The plugin protocol:
//!   stdin:  {"command": "observe"|"act", "target": ..., "handler": ..., "params": ..., "dry_run": ...}
//!   stdout: {"details": {...}} or {"error": {"code": "...", "message": "..."}}
//!
//! External plugins implement the same DomainPlugin trait as native plugins.
//! World dispatches through the trait — no separate code path.

use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::contracts::UnifiedResult;
use crate::plugin::{DispatchEntry, DomainPlugin};

/// A loaded external plugin.
#[derive(Debug)]
pub struct Plugin {
    pub domain: String,
    pub spec: Value,
    pub entries: Vec<DispatchEntry>,
    pub handler_path: PathBuf,
    pub session: bool,
}

#[derive(Debug, Deserialize)]
struct DispatchFile {
    entries: Vec<DispatchEntry>,
}

impl Plugin {
    /// Load a plugin from a directory.
    pub fn load(dir: &Path) -> Result<Self> {
        // Read spec.json
        let spec_path = dir.join("spec.json");
        let spec: Value = serde_json::from_str(&std::fs::read_to_string(&spec_path)?)?;
        let domain = spec["domain"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("spec.json missing 'domain' field"))?
            .to_string();

        // Read dispatch.json
        let dispatch_path = dir.join("dispatch.json");
        let dispatch_file: DispatchFile =
            serde_json::from_str(&std::fs::read_to_string(&dispatch_path)?)?;

        // Read session flag
        let session = spec["session"].as_bool().unwrap_or(false);

        // Find handler executable
        let handler_path = find_handler(dir)?;

        Ok(Plugin {
            domain,
            spec,
            entries: dispatch_file.entries,
            handler_path,
            session,
        })
    }

    /// Call the plugin handler via subprocess.
    async fn call_observe(
        &self,
        target: Option<&str>,
    ) -> Result<UnifiedResult> {
        let request = serde_json::json!({
            "command": "observe",
            "target": target,
        });
        self.call_handler(&request).await
    }

    /// Call the plugin handler for an act command.
    async fn call_act(
        &self,
        handler: &str,
        target: Option<&str>,
        params: Option<&Value>,
        dry_run: bool,
    ) -> Result<UnifiedResult> {
        let request = serde_json::json!({
            "command": "act",
            "handler": handler,
            "target": target,
            "params": params,
            "dry_run": dry_run,
        });
        self.call_handler(&request).await
    }

    async fn call_handler(&self, request: &Value) -> Result<UnifiedResult> {
        let input = serde_json::to_string(request)?;

        // Determine how to invoke: .py → python3, .sh → sh, otherwise direct
        let (program, args) = if self.handler_path.extension().map_or(false, |e| e == "py") {
            (
                "python3".to_string(),
                vec![self.handler_path.to_string_lossy().to_string()],
            )
        } else if self.handler_path.extension().map_or(false, |e| e == "js") {
            (
                "node".to_string(),
                vec![self.handler_path.to_string_lossy().to_string()],
            )
        } else if self.handler_path.extension().map_or(false, |e| e == "sh") {
            (
                "sh".to_string(),
                vec![self.handler_path.to_string_lossy().to_string()],
            )
        } else {
            (self.handler_path.to_string_lossy().to_string(), vec![])
        };

        let mut child = tokio::process::Command::new(&program)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        {
            use tokio::io::AsyncWriteExt;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input.as_bytes()).await?;
                stdin.shutdown().await?;
            }
        }

        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(UnifiedResult::err(
                "plugin_error",
                format!("Plugin '{}' failed: {}", self.domain, stderr.trim()),
            ));
        }

        let response: Value = serde_json::from_slice(&output.stdout)?;

        // Convert plugin response to UnifiedResult
        if let Some(err) = response.get("error") {
            let code = err["code"].as_str().unwrap_or("plugin_error");
            let message = err["message"].as_str().unwrap_or("Unknown error");
            Ok(UnifiedResult::err(code, message))
        } else {
            let details = response.get("details").cloned();
            Ok(UnifiedResult {
                output: String::new(),
                details,
                artifacts: None,
                error: None,
                risk: None,
                next_suggested_actions: None,
            })
        }
    }
}

// ─── DomainPlugin implementation ────────────────────────────────────────────

#[async_trait]
impl DomainPlugin for Plugin {
    fn domain(&self) -> &str {
        &self.domain
    }

    fn spec(&self) -> &Value {
        &self.spec
    }

    fn dispatch_entries(&self) -> &[DispatchEntry] {
        &self.entries
    }

    fn is_allowed(&self, _handler: &str) -> bool {
        // External plugins manage their own allowlists
        true
    }

    fn is_session(&self) -> bool {
        self.session
    }

    async fn observe(
        &self,
        target: Option<&str>,
        _since: Option<&str>,
        _limit: Option<u32>,
    ) -> Result<UnifiedResult> {
        self.call_observe(target).await
    }

    async fn act(
        &self,
        handler: &str,
        target: Option<&str>,
        params: Option<&Value>,
        dry_run: bool,
    ) -> Result<UnifiedResult> {
        self.call_act(handler, target, params, dry_run).await
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Find the handler executable in a plugin directory.
fn find_handler(dir: &Path) -> Result<PathBuf> {
    for candidate in &["handler.py", "handler.js", "handler.sh", "handler"] {
        let path = dir.join(candidate);
        if path.exists() {
            return Ok(path);
        }
    }
    Err(anyhow::anyhow!(
        "No handler found in {}. Expected handler.py, handler.js, handler.sh, or handler",
        dir.display()
    ))
}

/// Load all plugins from a directory. Each subdirectory is a potential plugin.
pub fn load_all(plugins_dir: &Path) -> Vec<Plugin> {
    let mut plugins = Vec::new();

    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(_) => return plugins,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("spec.json").exists() {
            match Plugin::load(&path) {
                Ok(plugin) => {
                    plugins.push(plugin);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: failed to load plugin at {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    plugins
}
