//! Multi-step record extraction pipeline
//!
//! Improves on single-shot extraction by breaking the process into steps:
//!
//! 1. Entity Extraction: Identify entities mentioned in the memo
//! 2. Record Matching: Search existing records for each entity
//! 3. Action Decision: Decide create/update/reference based on matches
//! 4. Execution: Apply the decided actions
//!
//! This approach is more reliable for updates because it does semantic
//! matching before deciding actions, rather than expecting the LLM to
//! cross-reference against a flat context list in one pass.

use anyhow::{Context, Result};
use llm::LlmClient;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::record::Record;
use crate::record_extraction::{
    ExtractedLink, ExtractedRecord, RecordAction, RecordExtractionResult,
};
use crate::Store;

/// An entity mention extracted from a memo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMention {
    /// The name as mentioned in the text
    pub name: String,
    /// Inferred type (repo, person, team, etc.)
    pub entity_type: String,
    /// Context around the mention
    pub context: String,
    /// Is this describing a change/update to something?
    pub is_update: bool,
    /// If update, what's the previous state?
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_value: Option<String>,
}

/// A candidate match for an entity
#[derive(Debug, Clone)]
pub struct RecordCandidate {
    pub record: Record,
    pub match_score: f32,
    pub match_reason: String,
}

/// Decision for what action to take on an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDecision {
    pub entity_name: String,
    pub entity_type: String,
    pub action: RecordAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub rationale: String,
}

/// Result of the entity extraction step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityExtractionResult {
    pub entities: Vec<EntityMention>,
    pub relationships: Vec<MentionedRelationship>,
}

/// A relationship mentioned in the memo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionedRelationship {
    pub source: String,
    pub target: String,
    pub relation: String,
}

/// Multi-step extractor
pub struct MultiStepExtractor<'a> {
    llm: &'a LlmClient,
    store: &'a Store,
}

impl<'a> MultiStepExtractor<'a> {
    pub fn new(llm: &'a LlmClient, store: &'a Store) -> Self {
        Self { llm, store }
    }

    /// Run the full multi-step extraction pipeline
    pub async fn extract(&self, memo_content: &str, memo_id: &str) -> Result<RecordExtractionResult> {
        info!("Starting multi-step extraction for memo {}", memo_id);

        // Step 1: Extract entity mentions
        let entity_result = self.extract_entities(memo_content).await?;
        debug!("Step 1: Found {} entities, {} relationships",
            entity_result.entities.len(),
            entity_result.relationships.len()
        );

        // Step 2: Match entities to existing records (parallel)
        let matches = self.match_entities(&entity_result.entities).await?;
        debug!("Step 2: Matched {} entities to existing records",
            matches.iter().filter(|(_, c)| !c.is_empty()).count()
        );

        // Step 3: Decide actions
        let decisions = self.decide_actions(memo_content, &entity_result, &matches).await?;
        debug!("Step 3: Made {} action decisions", decisions.len());

        // Step 4: Build final extraction result
        let result = self.build_result(&decisions, &entity_result.relationships)?;
        debug!("Step 4: Generated {} records, {} links",
            result.records.len(),
            result.links.len()
        );

        Ok(result)
    }

    /// Step 1: Extract entity mentions from the memo
    async fn extract_entities(&self, content: &str) -> Result<EntityExtractionResult> {
        let prompt = r#"Analyze this memo and extract ALL entity mentions and relationships.

ENTITY TYPES (ONLY use these six types):
- company: Organizations, businesses (e.g., "DataFlow Inc", "TechFlow")
- team: Groups within companies (e.g., "Engineering Team", "Pipeline Team")
- person: Individual people by name (e.g., "Sarah Kim", "Marcus Johnson")
  Note: Personal expertise like "Rust development" is an ATTRIBUTE of Person, not a separate entity
- repo: Code repositories (e.g., "flowcore", "dataflow-core") - NEVER use "document" for repos
- technology: Databases, frameworks, tools, cloud services - NEVER use "document"!
  Examples: PostgreSQL, SurrealDB, MySQL, MongoDB, AWS RDS, React, Python, Rust
- rule: Policies and guidelines - use the rule CONTENT as the name
  Example: "PRs require two reviews" → name: "Two code reviews required"

IMPORTANT - Do NOT create "skill" entities for personal expertise:
- "Elena has Rust expertise" → Person record for Elena (expertise noted in context)
- "Team knows React" → NOT a skill entity, just context about the team

CRITICAL:
1. Extract EVERY item from lists separately
2. PostgreSQL, SurrealDB, AWS RDS → type: "technology" (NEVER "document")
3. "Rust" when used as a programming language/tool = technology
4. For UPDATE detection: "switching from X to Y" → is_update=true, previous_value=X

RELATIONSHIPS - extract ALL mentioned:
- member_of: Person → Team ("led by Alice" → Alice member_of Team, "with Bob as engineer" → Bob member_of Team)
- belongs_to: Team → Company ("Team at Company" → Team belongs_to Company)
- owns: Team → Repo ("Team owns repo-x" → Team owns repo-x) - CREATE FOR EACH REPO!
- applies_to: Rule → Repo/Team ("applies to repo-x" → Rule applies_to repo-x)

For each entity:
- name: The canonical name (for rules, use rule content not labels)
- entity_type: One of the types above
- context: Brief context
- is_update: Is this describing a CHANGE? (switching from X to Y = update)
- previous_value: If is_update=true, what's being replaced?

Example 1 - Normal extraction:
{
  "entities": [
    {"name": "Pipeline Team", "entity_type": "team", "context": "owns repos", "is_update": false},
    {"name": "dataflow-core", "entity_type": "repo", "context": "core pipeline", "is_update": false}
  ],
  "relationships": [
    {"source": "Pipeline Team", "target": "dataflow-core", "relation": "owns"}
  ]
}

Example 2 - Update detection ("switching from PostgreSQL to SurrealDB"):
{
  "entities": [
    {"name": "SurrealDB", "entity_type": "technology", "context": "new database", "is_update": true, "previous_value": "PostgreSQL"}
  ],
  "relationships": []
}

Be thorough - extract EVERY entity and EVERY relationship mentioned!"#;

        let response = self.llm
            .complete(prompt, content)
            .await
            .context("Entity extraction LLM call failed")?;

        let json_str = extract_json(&response);
        let mut result: EntityExtractionResult = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse entity extraction: {}", json_str))?;

        // Post-process: correct common type misclassifications
        correct_entity_types(&mut result.entities);

        Ok(result)
    }

    /// Step 2: Match extracted entities to existing records
    async fn match_entities(&self, entities: &[EntityMention]) -> Result<Vec<(EntityMention, Vec<RecordCandidate>)>> {
        let mut results = Vec::new();

        for entity in entities {
            let candidates = self.find_matching_records(entity).await?;
            results.push((entity.clone(), candidates));
        }

        Ok(results)
    }

    /// Find existing records that might match an entity mention
    async fn find_matching_records(&self, entity: &EntityMention) -> Result<Vec<RecordCandidate>> {
        let mut candidates = Vec::new();

        // Try exact name match first
        if let Ok(records) = self.store.list_records(Some(&entity.entity_type), false, None).await {
            for record in records {
                let score = name_similarity(&entity.name, &record.name);
                if score > 0.5 {
                    candidates.push(RecordCandidate {
                        match_reason: format!("Name similarity: {:.0}%", score * 100.0),
                        match_score: score,
                        record,
                    });
                }
            }
        }

        // If this is an update, also check for the previous value
        if let Some(ref prev) = entity.previous_value {
            if let Ok(records) = self.store.list_records(Some(&entity.entity_type), false, None).await {
                for record in records {
                    let score = name_similarity(prev, &record.name);
                    if score > 0.7 {
                        candidates.push(RecordCandidate {
                            match_reason: format!("Matches previous value '{}': {:.0}%", prev, score * 100.0),
                            match_score: score,
                            record,
                        });
                    }
                }
            }
        }

        // Sort by score descending
        candidates.sort_by(|a, b| b.match_score.partial_cmp(&a.match_score).unwrap());

        Ok(candidates)
    }

    /// Step 3: Decide what actions to take
    async fn decide_actions(
        &self,
        memo_content: &str,
        entities: &EntityExtractionResult,
        matches: &[(EntityMention, Vec<RecordCandidate>)],
    ) -> Result<Vec<ActionDecision>> {
        // Build context for the LLM
        let mut match_context = String::new();
        for (entity, candidates) in matches {
            match_context.push_str(&format!("\n## Entity: {} ({})\n", entity.name, entity.entity_type));
            if entity.is_update {
                match_context.push_str(&format!("  UPDATE detected: replacing '{}'\n",
                    entity.previous_value.as_deref().unwrap_or("unknown")));
            }
            if candidates.is_empty() {
                match_context.push_str("  No existing records match.\n");
            } else {
                match_context.push_str("  Matching existing records:\n");
                for c in candidates.iter().take(3) {
                    match_context.push_str(&format!("    - {} (id: {}) - {}\n",
                        c.record.name,
                        c.record.id_str().unwrap_or_else(|| "?".to_string()),
                        c.match_reason
                    ));
                }
            }
        }

        let prompt = format!(r#"Based on the memo and entity matches, decide the action for each entity.

MEMO:
{}

ENTITY MATCHES:
{}

For each entity, decide:
- action: "create" (new entity), "update" (modify existing), or "reference" (just link to existing)
- existing_record_id: If update/reference, which record ID?
- new_name: If update changes the name (e.g., PostgreSQL→SurrealDB), the new name
- description: Updated description if applicable
- rationale: Why this action?

CRITICAL for updates:
- "Switching from X to Y" means UPDATE the X record to become Y
- "Promoted to new role" means UPDATE the person record with new role
- "Updated policy" means UPDATE the rule record

Output JSON array of decisions:
[
  {{
    "entity_name": "SurrealDB",
    "entity_type": "technology",
    "action": "update",
    "existing_record_id": "abc123",
    "new_name": "SurrealDB",
    "description": "Primary database (migrated from PostgreSQL)",
    "rationale": "Memo says 'switching from PostgreSQL to SurrealDB', so update existing tech record"
  }}
]"#, memo_content, match_context);

        let response = self.llm
            .complete(&prompt, "")
            .await
            .context("Action decision LLM call failed")?;

        let json_str = extract_json(&response);
        let mut decisions: Vec<ActionDecision> = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse action decisions: {}", json_str))?;

        // Post-process: correct common type misclassifications
        correct_decision_types(&mut decisions);

        Ok(decisions)
    }

    /// Step 4: Build the final extraction result
    fn build_result(
        &self,
        decisions: &[ActionDecision],
        relationships: &[MentionedRelationship],
    ) -> Result<RecordExtractionResult> {
        let records: Vec<ExtractedRecord> = decisions
            .iter()
            .map(|d| ExtractedRecord {
                action: d.action,
                existing_id: d.existing_record_id.clone(),
                record_type: d.entity_type.clone(),
                name: d.new_name.clone().unwrap_or_else(|| d.entity_name.clone()),
                description: d.description.clone(),
                content: serde_json::json!({}),
                confidence: 0.9,
            })
            .collect();

        let links: Vec<ExtractedLink> = relationships
            .iter()
            .map(|r| ExtractedLink {
                source: r.source.clone(),
                target: r.target.clone(),
                relation: r.relation.clone(),
                confidence: 0.9,
            })
            .collect();

        Ok(RecordExtractionResult {
            records,
            links,
            questions: vec![],
        })
    }
}

/// Simple name similarity using normalized Levenshtein-like comparison
fn name_similarity(a: &str, b: &str) -> f32 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    if a_lower == b_lower {
        return 1.0;
    }

    // Check if one contains the other
    if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
        return 0.8;
    }

    // Simple character overlap ratio
    let a_chars: std::collections::HashSet<_> = a_lower.chars().collect();
    let b_chars: std::collections::HashSet<_> = b_lower.chars().collect();
    let intersection = a_chars.intersection(&b_chars).count();
    let union = a_chars.union(&b_chars).count();

    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

/// Correct common entity type misclassifications
/// LLMs often output "document" for databases/tools that should be "technology"
fn correct_entity_types(entities: &mut [EntityMention]) {
    let tech_keywords = [
        "postgresql", "postgres", "surrealdb", "mysql", "mongodb", "redis",
        "elasticsearch", "aws", "rds", "s3", "dynamodb", "azure", "gcp",
    ];

    for entity in entities.iter_mut() {
        let name_lower = entity.name.to_lowercase();
        let type_lower = entity.entity_type.to_lowercase();

        // Document → Technology for known database/cloud terms
        // Use case-insensitive comparison since LLM may output "Document" vs "document"
        if type_lower == "document" || type_lower == "initiative" {
            if tech_keywords.iter().any(|kw| name_lower.contains(kw)) {
                entity.entity_type = "technology".to_string();
            }
        }
    }
}

/// Correct common entity type misclassifications in action decisions
/// This is needed because the LLM in decide_actions may also misclassify types
fn correct_decision_types(decisions: &mut [ActionDecision]) {
    let tech_keywords = [
        "postgresql", "postgres", "surrealdb", "mysql", "mongodb", "redis",
        "elasticsearch", "aws", "rds", "s3", "dynamodb", "azure", "gcp",
    ];

    for decision in decisions.iter_mut() {
        let name_lower = decision.entity_name.to_lowercase();
        let type_lower = decision.entity_type.to_lowercase();

        if type_lower == "document" || type_lower == "initiative" {
            if tech_keywords.iter().any(|kw| name_lower.contains(kw)) {
                decision.entity_type = "technology".to_string();
            }
        }
    }
}

/// Extract JSON from response that may be wrapped in markdown
fn extract_json(response: &str) -> &str {
    let trimmed = response.trim();

    if let Some(start) = trimmed.find("```") {
        let after_start = &trimmed[start + 3..];
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

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_similarity_exact() {
        assert_eq!(name_similarity("PostgreSQL", "PostgreSQL"), 1.0);
        assert_eq!(name_similarity("postgresql", "PostgreSQL"), 1.0);
    }

    #[test]
    fn test_name_similarity_contains() {
        assert!(name_similarity("Engineering Team", "Engineering") > 0.7);
    }

    #[test]
    fn test_name_similarity_different() {
        assert!(name_similarity("PostgreSQL", "SurrealDB") < 0.5);
    }

    #[test]
    fn test_correct_entity_types_postgresql() {
        let mut entities = vec![
            EntityMention {
                name: "PostgreSQL".to_string(),
                entity_type: "document".to_string(),
                context: "database".to_string(),
                is_update: false,
                previous_value: None,
            },
        ];
        correct_entity_types(&mut entities);
        assert_eq!(entities[0].entity_type, "technology", "PostgreSQL should be technology");
    }

    #[test]
    fn test_correct_entity_types_surrealdb() {
        let mut entities = vec![
            EntityMention {
                name: "SurrealDB".to_string(),
                entity_type: "document".to_string(),
                context: "new database".to_string(),
                is_update: false,
                previous_value: None,
            },
        ];
        correct_entity_types(&mut entities);
        assert_eq!(entities[0].entity_type, "technology", "SurrealDB should be technology");
    }

    #[test]
    fn test_correct_entity_types_aws_rds() {
        let mut entities = vec![
            EntityMention {
                name: "AWS RDS".to_string(),
                entity_type: "document".to_string(),
                context: "cloud database".to_string(),
                is_update: false,
                previous_value: None,
            },
        ];
        correct_entity_types(&mut entities);
        assert_eq!(entities[0].entity_type, "technology", "AWS RDS should be technology");
    }

    #[test]
    fn test_parse_entity_extraction_result() {
        let json = r#"{
            "entities": [
                {"name": "PostgreSQL", "entity_type": "document", "context": "db", "is_update": false}
            ],
            "relationships": []
        }"#;
        let result: EntityExtractionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.entities[0].entity_type, "document");
    }

    #[test]
    fn test_correct_entity_types_case_insensitive() {
        // LLM may output "Document" instead of "document"
        let mut entities = vec![
            EntityMention {
                name: "PostgreSQL".to_string(),
                entity_type: "Document".to_string(), // Capital D
                context: "database".to_string(),
                is_update: false,
                previous_value: None,
            },
        ];
        correct_entity_types(&mut entities);
        assert_eq!(entities[0].entity_type, "technology", "Should handle capitalized Document");
    }

    #[test]
    fn test_correct_decision_types() {
        use crate::RecordAction;
        let mut decisions = vec![
            ActionDecision {
                entity_name: "PostgreSQL".to_string(),
                entity_type: "document".to_string(),
                action: RecordAction::Create,
                existing_record_id: None,
                new_name: None,
                description: None,
                rationale: "test".to_string(),
            },
            ActionDecision {
                entity_name: "SurrealDB".to_string(),
                entity_type: "Document".to_string(), // Capital D
                action: RecordAction::Create,
                existing_record_id: None,
                new_name: None,
                description: None,
                rationale: "test".to_string(),
            },
        ];
        correct_decision_types(&mut decisions);
        assert_eq!(decisions[0].entity_type, "technology", "PostgreSQL should be technology");
        assert_eq!(decisions[1].entity_type, "technology", "SurrealDB should be technology");
    }
}
