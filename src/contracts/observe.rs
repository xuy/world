use serde::{Deserialize, Serialize};

/// Domains available for observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserveDomain {
    System,
    Network,
    Service,
    Process,
    Disk,
    Printer,
    Identity,
    Package,
    Log,
    Share,
    Security,
    Container,
    Certificate,
}

impl ObserveDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Network => "network",
            Self::Service => "service",
            Self::Process => "process",
            Self::Disk => "disk",
            Self::Printer => "printer",
            Self::Identity => "identity",
            Self::Package => "package",
            Self::Log => "log",
            Self::Share => "share",
            Self::Security => "security",
            Self::Container => "container",
            Self::Certificate => "certificate",
        }
    }
}

/// Arguments for the `observe` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserveArgs {
    pub domain: ObserveDomain,
    /// Specific target (e.g. service name, printer name, host).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Narrow the observation to specific scopes within the domain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
    /// Time filter for log-like observations (e.g. "1h", "30m").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Limit number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
