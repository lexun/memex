//! Record extraction pipeline for Atlas
//!
//! Extracts Records, Links, and Questions from episodes (memos, events).
//! This replaces the Entity/Fact extraction with a simpler model based on
//! the architecture decision (2026-01-18) to simplify to Records + Links.
//!
//! Key improvements over Entity/Fact extraction:
//! - Context-aware: Receives existing Records to avoid duplicates
//! - Uncertainty handling: Outputs Questions when confidence is low
//! - Direct to Records: No intermediate Entity tier

use anyhow::{Context, Result};
use llm::LlmClient;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::record::{EdgeRelation, RecordType};

/// Context provided to extraction for disambiguation
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExtractionContext {
    /// Existing records by type for reference
    pub repos: Vec<RecordSummary>,
    pub projects: Vec<RecordSummary>,
    pub people: Vec<RecordSummary>,
    pub teams: Vec<RecordSummary>,
    pub rules: Vec<RecordSummary>,
    pub companies: Vec<RecordSummary>,
}

/// Summary of an existing record for context injection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Result of extracting records from an episode
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordExtractionResult {
    /// Records to create or update
    pub records: Vec<ExtractedRecord>,
    /// Links to create between records
    pub links: Vec<ExtractedLink>,
    /// Questions for human clarification
    pub questions: Vec<ExtractionQuestion>,
}

/// A record extracted from content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRecord {
    /// What to do with this record
    pub action: RecordAction,
    /// ID of existing record (for update/reference actions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<String>,
    /// Record type
    pub record_type: String,
    /// Canonical name
    pub name: String,
    /// Description of what this represents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Type-specific content fields
    #[serde(default)]
    pub content: serde_json::Value,
    /// Extraction confidence (0.0-1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

/// Action to take for an extracted record
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordAction {
    /// Create a new record
    Create,
    /// Update an existing record
    Update,
    /// Reference an existing record (for linking)
    Reference,
}

impl Default for RecordAction {
    fn default() -> Self {
        RecordAction::Create
    }
}

/// A link between records
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedLink {
    /// Source record (name or existing_id)
    pub source: String,
    /// Target record (name or existing_id)
    pub target: String,
    /// Relationship type
    pub relation: String,
    /// Extraction confidence (0.0-1.0)
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

/// A question for human clarification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionQuestion {
    /// The question to ask
    pub text: String,
    /// Why this is ambiguous
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Possible answers if known
    #[serde(default)]
    pub options: Vec<String>,
    /// Source text that triggered the question
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
}

fn default_confidence() -> f32 {
    1.0
}

/// Result of processing extraction results
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionProcessingResult {
    /// Records that were created
    pub created_records: Vec<String>,
    /// Records that were updated
    pub updated_records: Vec<String>,
    /// Edges that were created
    pub created_edges: Vec<String>,
    /// Items skipped due to low confidence
    pub skipped_low_confidence: Vec<String>,
    /// Questions requiring human clarification
    pub questions: Vec<ExtractionQuestion>,
}

/// Record extractor using LLM with context awareness
pub struct RecordExtractor<'a> {
    llm: &'a LlmClient,
}

impl<'a> RecordExtractor<'a> {
    /// Create a new record extractor with the given LLM client
    pub fn new(llm: &'a LlmClient) -> Self {
        Self { llm }
    }

    /// Get a reference to the LLM client
    pub fn client(&self) -> &LlmClient {
        self.llm
    }

    /// Extract records, links, and questions from content
    pub async fn extract(
        &self,
        content: &str,
        context: &ExtractionContext,
        source_type: &str,
        source_id: &str,
    ) -> Result<RecordExtractionResult> {
        // Build the prompt with context
        let prompt = build_extraction_prompt(context);
        let user_content = format!(
            "Episode type: {}\nEpisode ID: {}\n\nContent:\n{}",
            source_type, source_id, content
        );

        let response = self
            .llm
            .complete(&prompt, &user_content)
            .await
            .context("LLM extraction failed")?;

        debug!("Raw LLM response: {}", response);

        // Parse JSON from response
        let json_str = extract_json(&response);
        let result: RecordExtractionResult = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse extraction response: {}", json_str))?;

        Ok(result)
    }

    /// Extract from a memo
    pub async fn extract_from_memo(
        &self,
        memo_content: &str,
        memo_id: &str,
        context: &ExtractionContext,
    ) -> Result<RecordExtractionResult> {
        self.extract(memo_content, context, "memo", memo_id).await
    }

    /// Extract from a task
    pub async fn extract_from_task(
        &self,
        task_content: &str,
        task_id: &str,
        context: &ExtractionContext,
    ) -> Result<RecordExtractionResult> {
        self.extract(task_content, context, "task", task_id).await
    }
}

/// Build the extraction prompt with context
fn build_extraction_prompt(context: &ExtractionContext) -> String {
    let mut context_section = String::from("## Existing Records in Knowledge Base\n\n");

    if !context.repos.is_empty() {
        context_section.push_str("**Repos:**\n");
        for r in &context.repos {
            context_section.push_str(&format!("- {} (id: {})\n", r.name, r.id));
        }
        context_section.push('\n');
    }

    if !context.projects.is_empty() {
        context_section.push_str("**Projects/Initiatives:**\n");
        for r in &context.projects {
            context_section.push_str(&format!("- {} (id: {})\n", r.name, r.id));
        }
        context_section.push('\n');
    }

    if !context.people.is_empty() {
        context_section.push_str("**People:**\n");
        for r in &context.people {
            context_section.push_str(&format!("- {} (id: {})\n", r.name, r.id));
        }
        context_section.push('\n');
    }

    if !context.teams.is_empty() {
        context_section.push_str("**Teams:**\n");
        for r in &context.teams {
            context_section.push_str(&format!("- {} (id: {})\n", r.name, r.id));
        }
        context_section.push('\n');
    }

    if !context.companies.is_empty() {
        context_section.push_str("**Companies:**\n");
        for r in &context.companies {
            context_section.push_str(&format!("- {} (id: {})\n", r.name, r.id));
        }
        context_section.push('\n');
    }

    if !context.rules.is_empty() {
        context_section.push_str("**Rules:**\n");
        for r in &context.rules {
            context_section.push_str(&format!("- {} (id: {})\n", r.name, r.id));
        }
        context_section.push('\n');
    }

    format!(
        r#"{context_section}
## Instructions

Extract knowledge from the episode below and output structured JSON.

**RECORDS**: Create records for ALL significant entities mentioned.
- Record types: repo, person, team, company, rule, skill, document, initiative
- If an entity clearly matches an existing record above, use action="reference" with existing_id
- If updating info about an existing record, use action="update" with existing_id
- Only use action="create" for genuinely new entities not in the list above

**IMPORTANT - Always create records for:**
- REPOS: Any code repository mentioned (e.g., "acme-api", "frontend-app"). Create even if only mentioned in passing.
- PEOPLE: Any person mentioned by name
- TEAMS: Any team or group mentioned
- COMPANIES: Any organization mentioned

**RULES**: When you identify a preference, guideline, or convention, create a Rule record.
- PRESERVE EXACT WORDING: Use the original phrasing from the source as the rule name
- If source says "Two approvals required", name it "Two approvals for PRs" not "Pull request approval requirement"
- If source says "Conventional commit format", name it "Conventional commits required" not "Commit message formatting"
- Keep the original terminology - don't paraphrase or use synonyms
- Include the full rule text in the description field

**LINKS**: Connect records with relationships. CREATE ALL APPLICABLE LINKS:
- member_of: Person is a member of a Team (includes leaders - "led by Alice" means Alice member_of Team)
- belongs_to: Team belongs to a Company; Repo belongs to an Organization
- owns: Team owns a Repo
- applies_to: Rule applies to a Repo or Team. "applies to all repos" means create applies_to link for EACH repo.
- related_to: General relationship
- depends_on: Technical dependency

CRITICAL:
- When text says "led by Alice", create: Alice member_of Team
- When text says "Team owns repo-name", create: Team owns repo-name
- When text says "applies to all repos", create applies_to link to EACH repo mentioned
- Don't skip links just because they seem obvious - create them explicitly

**QUESTIONS**: When you cannot confidently resolve a reference, create a question.
- Low confidence (<0.5) items should become questions instead of records

**Confidence levels:**
- High (0.8-1.0): Explicit statements, clear matches to existing records
- Medium (0.5-0.8): Reasonable inferences, likely matches
- Low (<0.5): Don't create record, create a question instead

Output JSON only (no markdown code blocks, no explanation):
{{
  "records": [
    {{
      "action": "create" | "update" | "reference",
      "existing_id": "only if action is update or reference",
      "record_type": "rule|repo|person|team|company|initiative|skill|document",
      "name": "Canonical name (preserve original wording for rules)",
      "description": "What this represents or the full rule text",
      "content": {{}},
      "confidence": 0.0-1.0
    }}
  ],
  "links": [
    {{
      "source": "record name or existing_id",
      "target": "record name or existing_id",
      "relation": "applies_to|belongs_to|member_of|owns|available_to|depends_on|related_to",
      "confidence": 0.0-1.0
    }}
  ],
  "questions": [
    {{
      "text": "What needs clarification?",
      "context": "Why this is ambiguous",
      "options": ["possible answer 1", "possible answer 2"],
      "source_text": "The ambiguous text from the episode"
    }}
  ]
}}

If the episode contains no extractable records or is purely operational, return empty arrays."#
    )
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

/// Validate extracted record type
pub fn parse_record_type(s: &str) -> Result<RecordType, String> {
    s.parse()
}

/// Validate extracted edge relation
pub fn parse_edge_relation(s: &str) -> Result<EdgeRelation, String> {
    s.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"records": [], "links": [], "questions": []}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_code_block() {
        let input = r#"```json
{"records": [], "links": [], "questions": []}
```"#;
        assert_eq!(
            extract_json(input),
            r#"{"records": [], "links": [], "questions": []}"#
        );
    }

    #[test]
    fn test_default_extraction_result() {
        let result = RecordExtractionResult::default();
        assert!(result.records.is_empty());
        assert!(result.links.is_empty());
        assert!(result.questions.is_empty());
    }

    #[test]
    fn test_parse_extraction_result() {
        let json = r#"{
            "records": [
                {
                    "action": "create",
                    "record_type": "rule",
                    "name": "Test Rule",
                    "description": "A test rule",
                    "content": {},
                    "confidence": 0.9
                }
            ],
            "links": [],
            "questions": []
        }"#;
        let result: RecordExtractionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].name, "Test Rule");
        assert_eq!(result.records[0].action, RecordAction::Create);
    }
}
