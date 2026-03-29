//! Phase 0 (v0.5): golden JSON under `templates/` must stay parseable and valid.
use peerclaw::crew::CrewSpec;
use peerclaw::flow::FlowSpec;

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

#[test]
fn templates_crews_minimal_json() {
    let p = format!("{}/templates/crews/minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let spec: CrewSpec = serde_json::from_str(&raw).expect("parse CrewSpec");
    spec.validate().expect("CrewSpec::validate");
}

#[test]
fn templates_crews_kickoff_minimal_json() {
    let p = format!("{}/templates/crews/kickoff-minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse JSON");
    let spec_value = v.get("spec").expect("kickoff body.spec");
    let spec: CrewSpec = serde_json::from_value(spec_value.clone()).expect("CrewSpec from spec");
    spec.validate().expect("nested CrewSpec::validate");
}

#[test]
fn templates_flows_minimal_json() {
    let p = format!("{}/templates/flows/minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let spec: FlowSpec = serde_json::from_str(&raw).expect("parse FlowSpec");
    spec.validate().expect("FlowSpec::validate");
    spec.execution_order().expect("FlowSpec::execution_order");
}

#[test]
fn templates_flows_interpreter_linear_json() {
    let p = format!("{}/templates/flows/interpreter-linear.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let spec: FlowSpec = serde_json::from_str(&raw).expect("parse FlowSpec");
    spec.validate_for_run().expect("FlowSpec::validate_for_run");
}

#[test]
fn flow_from_crew_conversion() {
    let p = format!("{}/templates/crews/minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let crew: CrewSpec = serde_json::from_str(&raw).expect("parse CrewSpec");
    let flow = FlowSpec::from_crew(crew);
    flow.validate_for_run().expect("converted flow should validate");
    assert!(flow.has_interpreter_start(), "converted flow should use interpreter mode");
}

#[test]
fn flow_single_agent() {
    let flow = FlowSpec::single_agent("Test");
    flow.validate_for_run().expect("single agent flow should validate");
    assert!(flow.has_interpreter_start());
}
