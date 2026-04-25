//! Workflow visualization — outputs workflow graphs as Mermaid, DOT, or JSON.
//!
//! Usage: agentic-loop visualize <workflow> [--format mermaid|dot|json]

use agentic_loop_types::workflow::Workflow;
use anyhow::Result;

/// Output format for visualization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisFormat {
    Mermaid,
    Dot,
    Json,
}

impl Default for VisFormat {
    fn default() -> Self {
        Self::Mermaid
    }
}

impl VisFormat {
    pub fn from_str(s: &str) -> Self {
        match s {
            "dot" => Self::Dot,
            "json" => Self::Json,
            _ => Self::Mermaid,
        }
    }
}

/// Visualize a workflow in the given format.
pub fn visualize(workflow: &Workflow, format: VisFormat) -> Result<String> {
    match format {
        VisFormat::Mermaid => Ok(to_mermaid(workflow)),
        VisFormat::Dot => Ok(to_dot(workflow)),
        VisFormat::Json => Ok(serde_json::to_string_pretty(workflow)?),
    }
}

/// Generate Mermaid flowchart from a workflow.
fn to_mermaid(workflow: &Workflow) -> String {
    let mut lines = Vec::new();
    lines.push("flowchart TD".to_string());
    lines.push(format!("    title[\"{}\"]:::title", escape_mermaid(&workflow.name)));
    lines.push("    classDef title fill:#4a90d9,color:#fff,font-weight:bold".to_string());

    // Add state nodes
    for state in &workflow.states {
        let label = escape_mermaid(&state.name);
        lines.push(format!("    {}[\"{}\"]", sanitize_id(&state.name), label));
    }

    // Add done node if not present
    if !workflow.states.iter().any(|s| s.name == "done") {
        lines.push("    done(((\"done\")))".to_string());
    }

    // Add failed node
    lines.push(r#"    failed((("failed")))"#.to_string());
    lines.push(String::new());

    // Add initial state indicator
    lines.push(format!("    START(( _ )) --> {}", sanitize_id(&workflow.initial_state)));
    lines.push(String::new());

    // Add transitions
    for transition in &workflow.transitions {
        let from = sanitize_id(&transition.from);
        let to = sanitize_id(&transition.to);
        let label = transition.condition.as_deref().unwrap_or("");
        if label.is_empty() {
            lines.push(format!("    {} --> {}", from, to));
        } else {
            lines.push(format!("    {} -->|\"{}\"| {}", from, escape_mermaid(label), to));
        }
    }

    // Add wildcard transitions
    for transition in &workflow.transitions {
        if transition.from == "*" {
            let to = sanitize_id(&transition.to);
            lines.push(format!("    * --> {}", to));
        }
    }

    lines.join("\n")
}

/// Generate DOT (Graphviz) from a workflow.
fn to_dot(workflow: &Workflow) -> String {
    let mut lines = Vec::new();
    lines.push(format!("digraph \"{}\" {{", escape_dot(&workflow.name)));
    lines.push("    rankdir=TD;".to_string());
    lines.push("    node [shape=box, style=rounded];".to_string());
    lines.push(format!("    label=\"{}\";", escape_dot(&workflow.name)));
    lines.push(String::new());

    // Start node
    lines.push("    START [shape=circle, label=\"\", width=0.3];".to_string());
    lines.push(format!("    START -> \"{}\";", escape_dot(&workflow.initial_state)));
    lines.push(String::new());

    // State nodes
    for state in &workflow.states {
        let label = escape_dot(&state.name);
        let shape = if state.name == "done" { "doublecircle" } else { "box" };
        lines.push(format!("    \"{}\" [shape={}, label=\"{}\"];", label, shape, label));
    }

    // Terminal nodes
    lines.push("    done [shape=doublecircle];".to_string());
    lines.push("    failed [shape=doublecircle, color=red];".to_string());
    lines.push(String::new());

    // Transitions
    for transition in &workflow.transitions {
        let from = escape_dot(&transition.from);
        let to = escape_dot(&transition.to);
        let label = transition.condition.as_deref().unwrap_or("");
        if label.is_empty() {
            lines.push(format!("    \"{}\" -> \"{}\";", from, to));
        } else {
            lines.push(format!("    \"{}\" -> \"{}\" [label=\"{}\"];", from, to, escape_dot(label)));
        }
    }

    lines.push("}".to_string());
    lines.join("\n")
}

fn sanitize_id(name: &str) -> String {
    name.replace('-', "_").replace(' ', "_")
}

fn escape_mermaid(s: &str) -> String {
    s.replace('"', "&quot;").replace('\n', " ")
}

fn escape_dot(s: &str) -> String {
    s.replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentic_loop_types::workflow::{State, Transition};

    fn make_workflow() -> Workflow {
        Workflow {
            name: "development".to_string(),
            description: "Dev workflow".to_string(),
            initial_state: "brainstorm".to_string(),
            states: vec![
                State { name: "brainstorm".to_string(), description: "Explore ideas".to_string(), config: serde_json::Value::Null },
                State { name: "implement".to_string(), description: "Write code".to_string(), config: serde_json::Value::Null },
                State { name: "test".to_string(), description: "Verify".to_string(), config: serde_json::Value::Null },
                State { name: "done".to_string(), description: "Complete".to_string(), config: serde_json::Value::Null },
            ],
            scenarios: vec![],
            transitions: vec![
                Transition { from: "brainstorm".to_string(), to: "implement".to_string(), condition: Some("auto".to_string()) },
                Transition { from: "implement".to_string(), to: "test".to_string(), condition: Some("auto".to_string()) },
                Transition { from: "test".to_string(), to: "done".to_string(), condition: Some("manual (done)".to_string()) },
                Transition { from: "*".to_string(), to: "failed".to_string(), condition: Some("manual (failed)".to_string()) },
            ],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::Value::Null,
            includes: vec![],
            delegates: vec![],
        }
    }

    #[test]
    fn test_mermaid() {
        let wf = make_workflow();
        let output = to_mermaid(&wf);
        assert!(output.contains("flowchart TD"));
        assert!(output.contains("brainstorm"));
        assert!(output.contains("implement"));
        assert!(output.contains("START(( _ )) --> brainstorm"));
        assert!(output.contains("brainstorm -->|\"auto\"| implement"));
    }

    #[test]
    fn test_dot() {
        let wf = make_workflow();
        let output = to_dot(&wf);
        assert!(output.contains("digraph"));
        assert!(output.contains("brainstorm"));
        assert!(output.contains("START -> \"brainstorm\""));
        assert!(output.contains("rankdir=TD"));
    }

    #[test]
    fn test_json_format() {
        let wf = make_workflow();
        let output = visualize(&wf, VisFormat::Json).unwrap();
        assert!(output.contains("brainstorm"));
        assert!(output.contains("development"));
    }

    #[test]
    fn test_format_from_str() {
        assert_eq!(VisFormat::from_str("mermaid"), VisFormat::Mermaid);
        assert_eq!(VisFormat::from_str("dot"), VisFormat::Dot);
        assert_eq!(VisFormat::from_str("json"), VisFormat::Json);
        assert_eq!(VisFormat::from_str("unknown"), VisFormat::Mermaid);
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("my-state"), "my_state");
        assert_eq!(sanitize_id("plain"), "plain");
    }

    #[test]
    fn test_visualize_default_is_mermaid() {
        let wf = make_workflow();
        let output = visualize(&wf, VisFormat::default()).unwrap();
        assert!(output.contains("flowchart TD"));
    }
}
