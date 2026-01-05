//! Fact extraction pipeline for Atlas
//!
//! Extracts structured facts and entities from episodes (memos, events)
//! using an LLM.

use anyhow::{Context, Result};
use llm::LlmClient;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::event::Event;
use crate::fact::{Entity, EntityType, EpisodeRef, Fact, FactType};
use crate::memo::Memo;

/// Result of extracting facts from an episode
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub facts: Vec<Fact>,
    pub entities: Vec<Entity>,
}

/// Raw extraction response from LLM (before processing)
#[derive(Debug, Deserialize)]
struct RawExtractionResponse {
    #[serde(default)]
    facts: Vec<RawFact>,
    #[serde(default)]
    entities: Vec<RawEntity>,
}

#[derive(Debug, Deserialize)]
struct RawFact {
    content: String,
    #[serde(default)]
    fact_type: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

#[derive(Debug, Deserialize)]
struct RawEntity {
    name: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    aliases: Vec<String>,
}

fn default_confidence() -> f32 {
    1.0
}

/// Fact extractor using LLM
pub struct Extractor {
    llm: LlmClient,
}

impl Extractor {
    /// Create a new extractor with the given LLM client
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// Get a reference to the LLM client
    pub fn client(&self) -> &LlmClient {
        &self.llm
    }

    /// Extract facts and entities from a memo
    pub async fn extract_from_memo(
        &self,
        memo: &Memo,
        project: Option<&str>,
    ) -> Result<ExtractionResult> {
        let episode_ref = EpisodeRef {
            episode_type: "memo".to_string(),
            episode_id: memo.id_str().unwrap_or_default(),
        };

        self.extract(&memo.content, episode_ref, project).await
    }

    /// Extract facts and entities from an event
    pub async fn extract_from_event(
        &self,
        event: &Event,
        project: Option<&str>,
    ) -> Result<ExtractionResult> {
        let episode_ref = EpisodeRef {
            episode_type: "event".to_string(),
            episode_id: event.id_str().unwrap_or_default(),
        };

        // Build text content from event
        let content = format!(
            "Event type: {}\nPayload: {}",
            event.event_type,
            serde_json::to_string_pretty(&event.payload).unwrap_or_default()
        );

        self.extract(&content, episode_ref, project).await
    }

    /// Extract facts and entities from arbitrary text
    async fn extract(
        &self,
        content: &str,
        episode_ref: EpisodeRef,
        project: Option<&str>,
    ) -> Result<ExtractionResult> {
        let response = self
            .llm
            .complete(EXTRACTION_PROMPT, content)
            .await
            .context("LLM extraction failed")?;

        debug!("Raw LLM response: {}", response);

        // Parse JSON from response (may be wrapped in markdown code block)
        let json_str = extract_json(&response);
        let raw: RawExtractionResponse = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse extraction response: {}", json_str))?;

        // Convert to proper types with provenance
        let facts = raw
            .facts
            .into_iter()
            .map(|f| {
                let fact_type = parse_fact_type(&f.fact_type);
                let mut fact = Fact::new(f.content, fact_type, f.confidence);
                fact.source_episodes.push(episode_ref.clone());
                if let Some(p) = project {
                    fact = fact.with_project(p);
                }
                fact
            })
            .collect();

        let entities = raw
            .entities
            .into_iter()
            .map(|e| {
                let entity_type = parse_entity_type(&e.entity_type);
                let mut entity = Entity::new(e.name, entity_type);
                entity.source_episodes.push(episode_ref.clone());
                entity.aliases = e.aliases;
                if let Some(p) = project {
                    entity = entity.with_project(p);
                }
                entity
            })
            .collect();

        Ok(ExtractionResult { facts, entities })
    }
}

/// Extract JSON from a response that may be wrapped in markdown code blocks
fn extract_json(response: &str) -> &str {
    let trimmed = response.trim();

    // Check for ```json ... ``` or ``` ... ```
    if let Some(start) = trimmed.find("```") {
        let after_start = &trimmed[start + 3..];
        // Skip optional "json" label
        let json_start = if after_start.starts_with("json") {
            after_start.find('\n').map(|i| i + 1).unwrap_or(0)
        } else if after_start.starts_with('\n') {
            1
        } else {
            0
        };
        let content = &after_start[json_start..];
        if let Some(end) = content.find("```") {
            return content[..end].trim();
        }
    }

    // No code block, return as-is
    trimmed
}

fn parse_fact_type(s: &str) -> FactType {
    match s.to_lowercase().as_str() {
        "decision" => FactType::Decision,
        "preference" => FactType::Preference,
        "question" => FactType::Question,
        "specification" | "spec" => FactType::Specification,
        _ => FactType::Statement,
    }
}

fn parse_entity_type(s: &str) -> EntityType {
    match s.to_lowercase().as_str() {
        "project" => EntityType::Project,
        "person" => EntityType::Person,
        "technology" | "tech" | "tool" | "library" => EntityType::Technology,
        "task" => EntityType::Task,
        "document" | "file" | "doc" => EntityType::Document,
        _ => EntityType::Concept,
    }
}

const EXTRACTION_PROMPT: &str = r#"You are a knowledge extraction system. Extract structured facts and entities from the input text.

FACTS are medium-granularity assertions - not too specific, not too vague. Each fact should:
- Be self-contained and understandable without the source text
- Capture a single assertion, decision, preference, question, or specification
- Include relevant context (e.g., "The memex project uses SurrealDB" not just "uses SurrealDB")

ENTITIES are named things mentioned in the text: projects, people, technologies, concepts, tasks, or documents.

Respond with JSON only (no markdown, no explanation):
{
  "facts": [
    {
      "content": "The fact as a complete assertion",
      "fact_type": "statement|decision|preference|question|specification",
      "confidence": 0.0-1.0
    }
  ],
  "entities": [
    {
      "name": "Canonical name",
      "entity_type": "project|person|technology|concept|task|document",
      "aliases": ["alternative", "names"]
    }
  ]
}

If the input contains no extractable facts or entities, return empty arrays.
Use high confidence (0.9-1.0) for explicit statements, lower (0.6-0.8) for inferences."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"facts": [], "entities": []}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_code_block() {
        let input = r#"```json
{"facts": [], "entities": []}
```"#;
        assert_eq!(extract_json(input), r#"{"facts": [], "entities": []}"#);
    }

    #[test]
    fn test_extract_json_code_block_no_label() {
        let input = r#"```
{"facts": [], "entities": []}
```"#;
        assert_eq!(extract_json(input), r#"{"facts": [], "entities": []}"#);
    }

    #[test]
    fn test_parse_fact_type() {
        assert_eq!(parse_fact_type("decision"), FactType::Decision);
        assert_eq!(parse_fact_type("PREFERENCE"), FactType::Preference);
        assert_eq!(parse_fact_type("unknown"), FactType::Statement);
    }

    #[test]
    fn test_parse_entity_type() {
        assert_eq!(parse_entity_type("project"), EntityType::Project);
        assert_eq!(parse_entity_type("tech"), EntityType::Technology);
        assert_eq!(parse_entity_type("library"), EntityType::Technology);
        assert_eq!(parse_entity_type("unknown"), EntityType::Concept);
    }
}
