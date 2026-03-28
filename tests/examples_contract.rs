//! Phase 0 (v0.5): golden JSON under `examples/` must stay parseable and valid.
use peerclaw::crew::CrewSpec;
use peerclaw::flow::FlowSpec;

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

#[test]
fn examples_crews_minimal_json() {
    let p = format!("{}/examples/crews/minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let spec: CrewSpec = serde_json::from_str(&raw).expect("parse CrewSpec");
    spec.validate().expect("CrewSpec::validate");
}

#[test]
fn examples_crews_kickoff_minimal_json() {
    let p = format!("{}/examples/crews/kickoff-minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse JSON");
    let spec_value = v.get("spec").expect("kickoff body.spec");
    let spec: CrewSpec = serde_json::from_value(spec_value.clone()).expect("CrewSpec from spec");
    spec.validate().expect("nested CrewSpec::validate");
}

#[test]
fn examples_flows_minimal_json() {
    let p = format!("{}/examples/flows/minimal.json", manifest_dir());
    let raw = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    let spec: FlowSpec = serde_json::from_str(&raw).expect("parse FlowSpec");
    spec.validate().expect("FlowSpec::validate");
    spec.execution_order().expect("FlowSpec::execution_order");
}
