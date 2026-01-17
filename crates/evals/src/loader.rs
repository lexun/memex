//! TOML scenario loader
//!
//! Loads evaluation scenarios from external TOML files.
//! Supports a human-friendly format that differs from the internal Scenario struct.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::scenario::{Action, AnswerType, ExpectedOutcome, Query, Scenario};

/// External TOML scenario format (human-friendly)
#[derive(Debug, Deserialize)]
struct TomlScenario {
    scenario: ScenarioMeta,
    setup: Vec<TomlAction>,
    queries: HashMap<String, TomlQuery>,
    #[serde(default)]
    evaluation: Option<EvaluationConfig>,
}

#[derive(Debug, Deserialize)]
struct ScenarioMeta {
    name: String,
    description: String,
    #[serde(default)]
    tests: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TomlAction {
    action: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    task_title: Option<String>,
    #[serde(default)]
    delay_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TomlQuery {
    query: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    should_find: Vec<String>,
    #[serde(default)]
    should_not_find: Vec<String>,
    #[serde(default)]
    expected_entities: Vec<String>,
    #[serde(default)]
    expected_behavior: Option<String>,
    #[serde(default)]
    answer_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EvaluationConfig {
    #[serde(default)]
    relevance_weight: Option<f32>,
    #[serde(default)]
    completeness_weight: Option<f32>,
    #[serde(default)]
    accuracy_weight: Option<f32>,
    #[serde(default)]
    coherence_weight: Option<f32>,
    #[serde(default)]
    notes: Option<String>,
}

/// Load a scenario from a TOML file
pub fn load_scenario(path: &Path) -> Result<Scenario> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read scenario file: {}", path.display()))?;

    let toml_scenario: TomlScenario = toml::from_str(&content)
        .with_context(|| format!("Failed to parse scenario file: {}", path.display()))?;

    convert_scenario(toml_scenario)
}

/// Load all scenarios from a directory
pub fn load_scenarios_from_dir(dir: &Path) -> Result<Vec<Scenario>> {
    let mut scenarios = Vec::new();

    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read scenarios directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "toml") {
            match load_scenario(&path) {
                Ok(scenario) => {
                    tracing::info!("Loaded scenario: {} from {}", scenario.name, path.display());
                    scenarios.push(scenario);
                }
                Err(e) => {
                    tracing::warn!("Failed to load {}: {}", path.display(), e);
                }
            }
        }
    }

    // Sort by name for consistent ordering
    scenarios.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(scenarios)
}

/// Convert from TOML format to internal Scenario format
fn convert_scenario(toml: TomlScenario) -> Result<Scenario> {
    let setup = toml
        .setup
        .into_iter()
        .map(convert_action)
        .collect::<Result<Vec<_>>>()?;

    let queries = toml
        .queries
        .into_iter()
        .map(|(name, q)| convert_query(name, q))
        .collect::<Vec<_>>();

    Ok(Scenario {
        name: toml.scenario.name,
        description: toml.scenario.description,
        setup,
        queries,
    })
}

/// Convert a TOML action to internal Action
fn convert_action(toml: TomlAction) -> Result<Action> {
    match toml.action.as_str() {
        "record_memo" => Ok(Action::RecordMemo {
            content: toml
                .content
                .ok_or_else(|| anyhow::anyhow!("record_memo requires content"))?,
            delay_secs: toml.delay_secs.unwrap_or(0),
        }),
        "create_task" => Ok(Action::CreateTask {
            title: toml
                .title
                .ok_or_else(|| anyhow::anyhow!("create_task requires title"))?,
            description: toml.description,
            project: toml.project,
        }),
        "add_note" => Ok(Action::AddTaskNote {
            task_title: toml
                .task_title
                .ok_or_else(|| anyhow::anyhow!("add_note requires task_title"))?,
            content: toml
                .content
                .ok_or_else(|| anyhow::anyhow!("add_note requires content"))?,
        }),
        "close_task" => Ok(Action::CloseTask {
            task_title: toml
                .task_title
                .ok_or_else(|| anyhow::anyhow!("close_task requires task_title"))?,
            reason: toml.content, // Use content as reason
        }),
        other => anyhow::bail!("Unknown action type: {}", other),
    }
}

/// Convert a TOML query to internal Query
fn convert_query(_name: String, toml: TomlQuery) -> Query {
    let answer_type = toml
        .answer_type
        .as_deref()
        .map(|s| match s.to_lowercase().as_str() {
            "factual" => AnswerType::Factual,
            "summary" => AnswerType::Summary,
            "list" => AnswerType::List,
            "explanation" => AnswerType::Explanation,
            _ => AnswerType::default(),
        })
        .unwrap_or_default();

    Query {
        query: toml.query,
        expected: ExpectedOutcome {
            should_mention: toml.should_find,
            should_not_mention: toml.should_not_find,
            answer_type,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_scenario() {
        let toml_str = r#"
[scenario]
name = "test-scenario"
description = "A test scenario"

[[setup]]
action = "record_memo"
content = "Test memo content"

[[setup]]
action = "create_task"
title = "Test task"
project = "test-project"

[queries.basic]
query = "What is the test about?"
should_find = ["test", "memo"]
"#;

        let toml_scenario: TomlScenario = toml::from_str(toml_str).unwrap();
        let scenario = convert_scenario(toml_scenario).unwrap();

        assert_eq!(scenario.name, "test-scenario");
        assert_eq!(scenario.setup.len(), 2);
        assert_eq!(scenario.queries.len(), 1);
    }

    #[test]
    fn test_action_conversion() {
        let memo_action = TomlAction {
            action: "record_memo".to_string(),
            content: Some("Test content".to_string()),
            title: None,
            description: None,
            project: None,
            task_title: None,
            delay_secs: Some(5),
        };

        let action = convert_action(memo_action).unwrap();
        match action {
            Action::RecordMemo { content, delay_secs } => {
                assert_eq!(content, "Test content");
                assert_eq!(delay_secs, 5);
            }
            _ => panic!("Expected RecordMemo"),
        }
    }
}
