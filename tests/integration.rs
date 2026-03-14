use world::tool::Tool;
use world::adapters::Platform;
use serde_json::json;
use std::sync::Arc;

fn tools() -> Vec<Box<dyn Tool>> {
    let (tools, _telemetry) = world::create_tools_for_platform(Platform::MacOS);
    tools
}

fn tools_with_telemetry() -> (Vec<Box<dyn Tool>>, Arc<world::telemetry::TelemetryLog>) {
    world::create_tools_for_platform(Platform::MacOS)
}

fn find_tool<'a>(tools: &'a [Box<dyn Tool>], name: &str) -> &'a dyn Tool {
    tools.iter().find(|t| t.name() == name).map(|t| &**t).unwrap()
}

#[test]
fn test_five_tools_registered() {
    let tools = tools();
    assert_eq!(tools.len(), 5);

    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"observe"));
    assert!(names.contains(&"act"));
    assert!(names.contains(&"verify"));
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"handoff"));
}

#[test]
fn test_tool_definitions_valid_json() {
    let tools = tools();
    for tool in &tools {
        let schema = tool.input_schema();
        // Must be an object with "type": "object" and "properties"
        assert_eq!(schema["type"], "object", "Tool {} schema must be object type", tool.name());
        assert!(schema["properties"].is_object(), "Tool {} must have properties", tool.name());
        assert!(schema["required"].is_array(), "Tool {} must have required array", tool.name());
    }
}

#[test]
fn test_observe_safety_tier() {
    let tools = tools();
    let observe = find_tool(&tools, "observe");
    assert_eq!(observe.safety_tier(), world::tool::SafetyTier::ReadOnly);
}

#[test]
fn test_act_safety_tiers() {
    let tools = tools();
    let act = find_tool(&tools, "act");

    // Dry run should be ReadOnly
    let dry_run_input = json!({"domain": "network", "action": "flush_dns", "dry_run": true});
    assert_eq!(
        act.safety_tier_for_input(&dry_run_input),
        world::tool::SafetyTier::ReadOnly,
    );

    // Low-risk action should be SafeAction
    let low_risk = json!({"domain": "network", "action": "flush_dns"});
    assert_eq!(
        act.safety_tier_for_input(&low_risk),
        world::tool::SafetyTier::SafeAction,
    );

    // High-risk action should need approval
    let high_risk = json!({"domain": "package", "action": "install_package", "target": "foo"});
    assert_eq!(
        act.safety_tier_for_input(&high_risk),
        world::tool::SafetyTier::NeedsApproval,
    );
}

#[test]
fn test_verify_safety_tier() {
    let tools = tools();
    let verify = find_tool(&tools, "verify");
    assert_eq!(verify.safety_tier(), world::tool::SafetyTier::ReadOnly);
}

#[test]
fn test_bash_safety_tiers() {
    let tools = tools();
    let bash = find_tool(&tools, "bash");

    // Safe command
    let safe = json!({"command": "echo hello", "reason": "testing"});
    assert_eq!(
        bash.safety_tier_for_input(&safe),
        world::tool::SafetyTier::SafeAction,
    );

    // Dangerous command
    let dangerous = json!({"command": "sudo rm -rf /tmp/foo", "reason": "cleanup"});
    assert_eq!(
        bash.safety_tier_for_input(&dangerous),
        world::tool::SafetyTier::NeedsApproval,
    );
}

#[tokio::test]
async fn test_observe_progressive_disclosure() {
    let tools = tools();
    let observe = find_tool(&tools, "observe");

    // Calling observe with just domain and no scope/target returns capabilities
    let input = json!({"domain": "network"});
    let result = observe.execute(&input).await.unwrap();

    // Should contain capability metadata
    let data = &result.data;
    assert!(data["details"]["allowed_scopes"].is_array());
    assert!(data["details"]["related_actions"].is_array());
    assert!(data["details"]["related_verifications"].is_array());
}

#[tokio::test]
async fn test_handoff_creates_summary() {
    let tools = tools();
    let handoff = find_tool(&tools, "handoff");

    let input = json!({
        "summary": "Printer is offline and host is unreachable. Likely physical or network issue.",
        "severity": "medium",
        "recommended_human_owner": "it_admin",
        "evidence_refs": ["observe_printer_1", "verify_host_reachable_1"],
    });

    let result = handoff.execute(&input).await.unwrap();
    assert!(result.output.contains("Handoff Summary"));
    assert!(result.output.contains("Medium"));
}

#[test]
fn test_policy_risk_classification() {
    use world::contracts::act::ActDomain;
    use world::contracts::Risk;
    use world::policy;

    assert_eq!(policy::classify_risk(ActDomain::Network, "flush_dns"), Risk::Low);
    assert_eq!(policy::classify_risk(ActDomain::Service, "restart_service"), Risk::Medium);
    assert_eq!(policy::classify_risk(ActDomain::Package, "install_package"), Risk::High);
}

#[test]
fn test_policy_allowlist() {
    use world::contracts::act::ActDomain;
    use world::policy;

    assert!(policy::is_allowed(ActDomain::Network, "flush_dns"));
    assert!(policy::is_allowed(ActDomain::Printer, "clear_queue"));
    assert!(!policy::is_allowed(ActDomain::Network, "delete_everything"));
}

#[test]
fn test_recommended_verifications() {
    use world::policy;

    let verifs = policy::recommended_verifications("flush_dns");
    assert!(verifs.contains(&"dns_resolves".to_string()));
    assert!(verifs.contains(&"internet_reachable".to_string()));

    let verifs = policy::recommended_verifications("restart_service");
    assert!(verifs.contains(&"service_healthy".to_string()));
}

// ── Phase 4: Anthropic API tool definition serialization ──────────────

#[test]
fn test_tool_definitions_serialize_to_anthropic_format() {
    let tools = tools();
    for tool in &tools {
        let def = json!({
            "name": tool.name(),
            "description": tool.description(),
            "input_schema": tool.input_schema(),
        });
        // Must have all required Anthropic API fields
        assert!(def["name"].is_string(), "Tool {} must have string name", tool.name());
        assert!(def["description"].is_string(), "Tool {} must have string description", tool.name());
        assert!(def["input_schema"]["type"].as_str() == Some("object"),
            "Tool {} input_schema must be object type", tool.name());
        assert!(def["input_schema"]["properties"].is_object(),
            "Tool {} must have properties", tool.name());

        // Verify JSON round-trip works
        let serialized = serde_json::to_string(&def).unwrap();
        let _deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    }
}

// ── Phase 4: observe → act → verify flow ──────────────────────

#[tokio::test]
async fn test_observe_network_with_scope() {
    let tools = tools();
    let observe = find_tool(&tools, "observe");

    // Calling observe with a specific scope should return real data (not capabilities)
    let input = json!({"domain": "network", "scope": ["interfaces", "internet_status"]});
    let result = observe.execute(&input).await.unwrap();

    // Should have output text and structured data
    assert!(!result.output.is_empty(), "observe(network) should produce output");
    assert!(result.data.is_object(), "observe(network) should produce structured data");
}

#[tokio::test]
async fn test_act_dry_run() {
    let tools = tools();
    let act = find_tool(&tools, "act");

    // Dry run should describe what would happen without doing it
    let input = json!({"domain": "network", "action": "flush_dns", "dry_run": true});
    let result = act.execute(&input).await.unwrap();

    assert!(result.output.contains("Would"), "dry_run should say 'Would'");
    assert!(result.changes.is_empty(), "dry_run should have no changes");
}

#[tokio::test]
async fn test_act_disallowed_action() {
    let tools = tools();
    let act = find_tool(&tools, "act");

    let input = json!({"domain": "network", "action": "delete_everything"});
    let result = act.execute(&input).await.unwrap();

    assert!(result.output.contains("not allowed"), "disallowed action should be rejected");
}

#[tokio::test]
async fn test_verify_internet_reachable() {
    let tools = tools();
    let verify = find_tool(&tools, "verify");

    let input = json!({"check": "internet_reachable", "timeout_sec": 5});
    let result = verify.execute(&input).await.unwrap();

    assert!(!result.output.is_empty(), "verify should produce output");
    // Should contain a "passed" field in the data
    let details = &result.data;
    assert!(details["details"]["check"].as_str() == Some("internet_reachable"));
    assert!(details["details"]["passed"].is_boolean());
}

// ── Phase 4: Telemetry trail works across tools ─────────────────────

#[tokio::test]
async fn test_telemetry_records_tool_calls() {
    let (tools, telemetry) = tools_with_telemetry();
    let observe = find_tool(&tools, "observe");

    // Call observe with scope to generate a real call (not progressive disclosure)
    let input = json!({"domain": "network", "scope": ["interfaces"]});
    let _ = observe.execute(&input).await.unwrap();

    let events = telemetry.events();
    assert!(!events.is_empty(), "telemetry should record the observe call");
    assert_eq!(events[0].tool, "observe");
    assert_eq!(events[0].domain, Some("network".to_string()));
}

// ── Phase 4: Progressive disclosure for all domains ─────────────────

#[tokio::test]
async fn test_progressive_disclosure_all_domains() {
    let tools = tools();
    let observe = find_tool(&tools, "observe");

    for domain in &["network", "service", "disk", "printer", "package", "log"] {
        let input = json!({"domain": domain});
        let result = observe.execute(&input).await.unwrap();
        let data = &result.data;
        assert!(
            data["details"]["allowed_scopes"].is_array(),
            "Progressive disclosure for {domain} should return allowed_scopes"
        );
    }
}

// ── Phase 4: Bash tool blocks dangerous commands ────────────────────

#[tokio::test]
async fn test_bash_blocks_dangerous_commands() {
    let tools = tools();
    let bash = find_tool(&tools, "bash");

    let input = json!({"command": "rm -rf /", "reason": "testing blocked command"});
    let result = bash.execute(&input).await.unwrap();
    assert!(result.output.contains("blocked"), "rm -rf / should be blocked");
}

#[tokio::test]
async fn test_bash_executes_safe_command() {
    let tools = tools();
    let bash = find_tool(&tools, "bash");

    let input = json!({"command": "echo hello_unified_tools", "reason": "smoke test"});
    let result = bash.execute(&input).await.unwrap();
    assert!(result.output.contains("hello_unified_tools"), "safe command should execute");
}

// ── Phase 4: ToolRouter compatibility ────────────────────────────────

#[test]
fn test_tool_router_compatibility() {
    // Verify unified tools can be registered with ToolRouter
    // (same interface as existing platform tools)
    let (tools, _telemetry) = world::create_tools();

    // Each tool must satisfy the Tool trait bounds
    for tool in &tools {
        let _name: &str = tool.name();
        let _desc: &str = tool.description();
        let _schema: serde_json::Value = tool.input_schema();
        let _tier: world::tool::SafetyTier = tool.safety_tier();
    }

    // Verify we get exactly 5 tools with correct names
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names, vec!["observe", "act", "verify", "bash", "handoff"]);
}
