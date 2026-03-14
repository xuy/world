use anyhow::Result;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
    since: Option<&str>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            crate::adapters::macos::log::observe(target, since, limit).await
        }
        _ => Ok(UnifiedResult::unsupported("log observation")),
    }
}
