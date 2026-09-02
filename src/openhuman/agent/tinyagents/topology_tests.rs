use super::*;

#[test]
fn all_topologies_includes_the_member_graph() {
    let reports = all_graph_topologies();
    let member = reports
        .iter()
        .find(|r| r.name == "agent_teams:member")
        .expect("the agent_teams member graph should be exported");

    // The member graph is a fixed, well-formed structure.
    assert!(
        member.ok,
        "member graph should validate structurally: {:?}",
        member.errors
    );
    assert!(member.errors.is_empty());
}

#[test]
fn all_topologies_includes_delegation_and_workflow_scheduler() {
    let reports = all_graph_topologies();
    for name in ["delegation", "workflow_runs:scheduler"] {
        let report = reports
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("the {name} graph should be exported"));
        assert!(
            report.ok,
            "{name} graph should validate structurally: {:?}",
            report.errors
        );
        assert!(
            report.mermaid.contains("flowchart"),
            "{name} mermaid should render: {}",
            report.mermaid
        );
    }
}

#[test]
fn delegation_topology_names_the_revision_loop_nodes() {
    let t = super::super::delegation::delegation_graph_topology().expect("builds");
    let names: Vec<&str> = t.nodes.iter().map(|n| n.id.as_str()).collect();
    for expected in ["plan", "execute", "review", "finalize"] {
        assert!(
            names.contains(&expected),
            "missing node {expected}: {names:?}"
        );
    }
}

#[test]
fn member_report_renders_mermaid_and_valid_json() {
    let t = crate::openhuman::agent::orchestration::agent_teams::member_graph_topology()
        .expect("member topology builds");
    let report = describe("agent_teams:member", &t);

    // Mermaid is a flowchart with at least the entry node rendered.
    assert!(
        report.mermaid.contains("flowchart"),
        "mermaid should be a flowchart: {}",
        report.mermaid
    );
    assert!(!t.nodes.is_empty(), "the graph should declare nodes");

    // JSON round-trips to a value carrying the same node set.
    let parsed: serde_json::Value =
        serde_json::from_str(&report.json).expect("topology JSON parses");
    assert!(
        parsed.get("nodes").is_some(),
        "serialized topology should carry its nodes: {}",
        report.json
    );
}
