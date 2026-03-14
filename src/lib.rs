//! World — POMDP interface for agents.
//!
//! A neutral framework. Every domain is a plugin:
//! - Native Rust plugins for performance (network, service, disk, ...)
//! - External subprocess plugins for extensibility (pip, ...)
//!
//! Core operations:
//! - `observe` — read-only structured system state (O)
//! - `act` — finite verbs on schema paths (A)
//!
//! The `DomainPlugin` trait is the unified interface that all plugins implement.

pub mod adapters;
pub mod awaiting;
pub mod cli;
pub mod contracts;
pub mod dispatch;
pub mod domains;
pub mod execution;
pub mod plugin;
pub mod policy;
pub mod sampling;
pub mod schemas;
pub mod spec;
pub mod telemetry;
pub mod tool;
pub mod tools;

use adapters::Platform;
use std::sync::Arc;
use telemetry::TelemetryLog;
use tool::Tool;

/// Create the 5 unified tools for the current platform.
pub fn create_tools() -> (Vec<Box<dyn Tool>>, Arc<TelemetryLog>) {
    let platform = Platform::current();
    let telemetry = Arc::new(TelemetryLog::new());
    let tools = tools::create_unified_tools(platform, telemetry.clone());
    (tools, telemetry)
}

/// Create tools for a specific platform (useful for testing).
pub fn create_tools_for_platform(platform: Platform) -> (Vec<Box<dyn Tool>>, Arc<TelemetryLog>) {
    let telemetry = Arc::new(TelemetryLog::new());
    let tools = tools::create_unified_tools(platform, telemetry.clone());
    (tools, telemetry)
}
