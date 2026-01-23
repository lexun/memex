//! LLM-based answer evaluation
//!
//! Uses an LLM to subjectively evaluate answer quality against expected outcomes.
//! Supports multiple evaluation dimensions including relevance, completeness,
//! accuracy, coherence, and specialized dimensions for extraction evaluation.

use anyhow::{Context, Result};
use llm::LlmClient;
use serde::{Deserialize, Serialize};

use crate::scenario::{AnswerType, ExpectedOutcome};

/// Evaluation scores for an answer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalScore {
    /// Does the answer address the question? (0-100)
    pub relevance: u8,
    /// Does it include all expected information? (0-100)
    pub completeness: u8,
    /// Is the information correct? (0-100)
    pub accuracy: u8,
    /// Is the answer well-structured? (0-100)
    pub coherence: u8,
    /// Overall score (0-100)
    pub overall: u8,
}

/// Extended evaluation scores for more detailed analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedEvalScore {
    /// Basic scores
    pub basic: EvalScore,
    /// Did the system correctly identify entities? (0-100)
    pub entity_recognition: Option<u8>,
    /// Did the system correctly identify relationships? (0-100)
    pub relationship_accuracy: Option<u8>,
    /// Did the system avoid hallucination? (0-100)
    pub factual_grounding: Option<u8>,
    /// Did the system handle updates/changes correctly? (0-100)
    pub temporal_reasoning: Option<u8>,
    /// Did the system correctly disambiguate similar entities? (0-100)
    pub disambiguation: Option<u8>,
}

impl EvalScore {
    /// Check if the score passes a threshold
    pub fn passes(&self, threshold: u8) -> bool {
        self.overall >= threshold
    }

    /// Calculate average of all dimensions
    pub fn average(&self) -> f32 {
        (self.relevance as f32
            + self.completeness as f32
            + self.accuracy as f32
            + self.coherence as f32)
            / 4.0
    }
}

/// Complete evaluation result for a query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    /// The query that was asked
    pub query: String,
    /// The answer that was returned
    pub answer: String,
    /// The expected outcome
    pub expected: ExpectedOutcome,
    /// The evaluation scores
    pub scores: EvalScore,
    /// Items that should have been mentioned but weren't
    pub missing_mentions: Vec<String>,
    /// Items that shouldn't have been mentioned but were
    pub incorrect_mentions: Vec<String>,
    /// Free-form feedback from the judge
    pub feedback: String,
}

impl EvalResult {
    /// Check if this result is a pass
    pub fn passed(&self) -> bool {
        self.scores.passes(70) && self.missing_mentions.is_empty()
    }
}

/// LLM-based judge for evaluating answers
pub struct Judge {
    llm: LlmClient,
}

impl Judge {
    /// Create a new judge with the given LLM client
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// Evaluate an answer against expected outcomes
    pub async fn evaluate(
        &self,
        query: &str,
        answer: &str,
        expected: &ExpectedOutcome,
    ) -> Result<EvalResult> {
        // Build the evaluation prompt
        let prompt = self.build_eval_prompt(query, answer, expected);

        // Get LLM evaluation
        let response = self
            .llm
            .complete(JUDGE_SYSTEM_PROMPT, &prompt)
            .await
            .context("LLM evaluation failed")?;

        // Parse the evaluation response
        let raw: RawEvalResponse = serde_json::from_str(&extract_json(&response))
            .with_context(|| format!("Failed to parse eval response: {}", response))?;

        // Check for missing/incorrect mentions
        let missing_mentions = self.find_missing_mentions(answer, &expected.should_mention);
        let incorrect_mentions =
            self.find_incorrect_mentions(answer, &expected.should_not_mention);

        Ok(EvalResult {
            query: query.to_string(),
            answer: answer.to_string(),
            expected: expected.clone(),
            scores: EvalScore {
                relevance: raw.relevance,
                completeness: raw.completeness,
                accuracy: raw.accuracy,
                coherence: raw.coherence,
                overall: raw.overall,
            },
            missing_mentions,
            incorrect_mentions,
            feedback: raw.feedback,
        })
    }

    /// Evaluate extraction quality with extended dimensions
    pub async fn evaluate_extraction(
        &self,
        memos: &[String],
        extracted_records: &[String],
        extracted_edges: &[String],
        expected_records: &[String],
        expected_edges: &[String],
    ) -> Result<ExtractionEvalResult> {
        let prompt = self.build_extraction_eval_prompt(
            memos,
            extracted_records,
            extracted_edges,
            expected_records,
            expected_edges,
        );

        let response = self
            .llm
            .complete(EXTRACTION_JUDGE_SYSTEM_PROMPT, &prompt)
            .await
            .context("LLM extraction evaluation failed")?;

        let raw: RawExtractionEvalResponse = serde_json::from_str(&extract_json(&response))
            .with_context(|| format!("Failed to parse extraction eval response: {}", response))?;

        Ok(ExtractionEvalResult {
            entity_recognition: raw.entity_recognition,
            relationship_accuracy: raw.relationship_accuracy,
            factual_grounding: raw.factual_grounding,
            temporal_reasoning: raw.temporal_reasoning,
            disambiguation: raw.disambiguation,
            overall: raw.overall,
            feedback: raw.feedback,
            missing_entities: raw.missing_entities,
            hallucinated_entities: raw.hallucinated_entities,
            relationship_errors: raw.relationship_errors,
        })
    }

    fn build_extraction_eval_prompt(
        &self,
        memos: &[String],
        extracted_records: &[String],
        extracted_edges: &[String],
        expected_records: &[String],
        expected_edges: &[String],
    ) -> String {
        let memos_str = memos.iter()
            .enumerate()
            .map(|(i, m)| format!("{}. {}", i + 1, m))
            .collect::<Vec<_>>()
            .join("\n");

        let extracted_records_str = extracted_records.join("\n");
        let extracted_edges_str = extracted_edges.join("\n");
        let expected_records_str = expected_records.join("\n");
        let expected_edges_str = expected_edges.join("\n");

        format!(
            r#"Evaluate the quality of entity and relationship extraction from these memos.

ORIGINAL MEMOS:
{memos_str}

EXTRACTED RECORDS:
{extracted_records_str}

EXTRACTED EDGES/RELATIONSHIPS:
{extracted_edges_str}

EXPECTED RECORDS:
{expected_records_str}

EXPECTED EDGES/RELATIONSHIPS:
{expected_edges_str}

Evaluate on these dimensions (0-100 scale):
- entity_recognition: Were all entities correctly identified?
- relationship_accuracy: Were relationships between entities correct?
- factual_grounding: Did extraction stay true to memo content without hallucination?
- temporal_reasoning: Were updates/changes handled correctly (if applicable)?
- disambiguation: Were similar entities correctly distinguished (if applicable)?

Also identify:
- missing_entities: Important entities not extracted
- hallucinated_entities: Entities extracted but not in source
- relationship_errors: Incorrect or missing relationships

Provide your evaluation as JSON."#
        )
    }

    fn build_eval_prompt(&self, query: &str, answer: &str, expected: &ExpectedOutcome) -> String {
        let answer_type_desc = match expected.answer_type {
            AnswerType::Factual => "a factual response with specific information",
            AnswerType::Summary => "a summary of multiple items",
            AnswerType::List => "a list of items",
            AnswerType::Explanation => "an explanation of a concept",
        };

        let should_mention = if expected.should_mention.is_empty() {
            "None specified".to_string()
        } else {
            expected.should_mention.join(", ")
        };

        let should_not_mention = if expected.should_not_mention.is_empty() {
            "None specified".to_string()
        } else {
            expected.should_not_mention.join(", ")
        };

        format!(
            r#"Evaluate this answer to a knowledge query.

QUERY: {query}

ANSWER: {answer}

EXPECTED ANSWER TYPE: {answer_type_desc}

KEY ITEMS THAT SHOULD BE MENTIONED: {should_mention}

ITEMS THAT SHOULD NOT BE MENTIONED: {should_not_mention}

Evaluate the answer on these dimensions (0-100 scale):
- Relevance: Does the answer address the question asked?
- Completeness: Does it include the key items that should be mentioned?
- Accuracy: Is the information presented accurately?
- Coherence: Is the answer well-structured and easy to understand?

Provide your evaluation as JSON."#
        )
    }

    fn find_missing_mentions(&self, answer: &str, should_mention: &[String]) -> Vec<String> {
        let answer_lower = answer.to_lowercase();
        should_mention
            .iter()
            .filter(|item| !answer_lower.contains(&item.to_lowercase()))
            .cloned()
            .collect()
    }

    fn find_incorrect_mentions(&self, answer: &str, should_not_mention: &[String]) -> Vec<String> {
        let answer_lower = answer.to_lowercase();
        should_not_mention
            .iter()
            .filter(|item| answer_lower.contains(&item.to_lowercase()))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct RawEvalResponse {
    relevance: u8,
    completeness: u8,
    accuracy: u8,
    coherence: u8,
    overall: u8,
    #[serde(default)]
    feedback: String,
}

#[derive(Debug, Deserialize)]
struct RawExtractionEvalResponse {
    entity_recognition: u8,
    relationship_accuracy: u8,
    factual_grounding: u8,
    #[serde(default)]
    temporal_reasoning: Option<u8>,
    #[serde(default)]
    disambiguation: Option<u8>,
    overall: u8,
    #[serde(default)]
    feedback: String,
    #[serde(default)]
    missing_entities: Vec<String>,
    #[serde(default)]
    hallucinated_entities: Vec<String>,
    #[serde(default)]
    relationship_errors: Vec<String>,
}

/// Result of LLM-based extraction evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionEvalResult {
    /// How well were entities identified? (0-100)
    pub entity_recognition: u8,
    /// How accurate were the relationships? (0-100)
    pub relationship_accuracy: u8,
    /// Did extraction stay grounded in facts? (0-100)
    pub factual_grounding: u8,
    /// Were temporal updates handled correctly? (0-100, optional)
    pub temporal_reasoning: Option<u8>,
    /// Were similar entities distinguished? (0-100, optional)
    pub disambiguation: Option<u8>,
    /// Overall extraction quality (0-100)
    pub overall: u8,
    /// Detailed feedback
    pub feedback: String,
    /// Entities that were expected but not extracted
    pub missing_entities: Vec<String>,
    /// Entities that were extracted but not in source
    pub hallucinated_entities: Vec<String>,
    /// Relationship errors identified
    pub relationship_errors: Vec<String>,
}

impl ExtractionEvalResult {
    /// Check if the extraction passed quality thresholds
    pub fn passed(&self) -> bool {
        self.overall >= 70
            && self.entity_recognition >= 60
            && self.relationship_accuracy >= 60
            && self.factual_grounding >= 70
    }
}

/// Extract JSON from a response that may be wrapped in markdown code blocks
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

const JUDGE_SYSTEM_PROMPT: &str = r#"You are an evaluation judge for a knowledge management system. Your task is to evaluate how well the system answered a user's query.

Score each dimension from 0-100:
- 0-30: Poor - fails to meet basic expectations
- 31-50: Below average - significant gaps
- 51-70: Average - adequate but room for improvement
- 71-85: Good - meets expectations well
- 86-100: Excellent - exceeds expectations

Respond with JSON only:
{
  "relevance": <0-100>,
  "completeness": <0-100>,
  "accuracy": <0-100>,
  "coherence": <0-100>,
  "overall": <0-100>,
  "feedback": "Brief explanation of scores and suggestions for improvement"
}

Be strict but fair. An empty or "no information found" answer should score 0 on completeness if key items were expected."#;

const EXTRACTION_JUDGE_SYSTEM_PROMPT: &str = r#"You are an evaluation judge for a knowledge extraction system. Your task is to evaluate how well the system extracted entities and relationships from source memos.

Score each dimension from 0-100:
- 0-30: Poor - major errors or omissions
- 31-50: Below average - several mistakes
- 51-70: Average - mostly correct with some gaps
- 71-85: Good - accurate with minor issues
- 86-100: Excellent - comprehensive and precise

Key evaluation criteria:
1. entity_recognition: Did the system identify all important entities (people, teams, repos, technologies, rules)?
2. relationship_accuracy: Are the extracted relationships (belongs_to, member_of, owns, applies_to) correct?
3. factual_grounding: Did the system only extract information that was actually in the source memos, without hallucinating?
4. temporal_reasoning: If the memos contain updates or changes over time, did the system handle this correctly? (Only score if applicable)
5. disambiguation: If there were similar entities (e.g., two people named Kim), were they correctly distinguished? (Only score if applicable)

Respond with JSON only:
{
  "entity_recognition": <0-100>,
  "relationship_accuracy": <0-100>,
  "factual_grounding": <0-100>,
  "temporal_reasoning": <0-100 or null if not applicable>,
  "disambiguation": <0-100 or null if not applicable>,
  "overall": <0-100>,
  "feedback": "Brief explanation of scores",
  "missing_entities": ["entity1", "entity2"],
  "hallucinated_entities": ["entity3"],
  "relationship_errors": ["description of error 1", "description of error 2"]
}

Be thorough in identifying errors. Hallucination (making up entities or relationships not in the source) should severely penalize factual_grounding."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json() {
        let input = r#"```json
{"relevance": 80, "completeness": 70, "accuracy": 90, "coherence": 85, "overall": 81, "feedback": "Good answer"}
```"#;
        let json = extract_json(input);
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
    }

    #[test]
    fn test_find_missing_mentions() {
        let answer = "The system uses SurrealDB for storage";
        let should_mention = vec!["SurrealDB".to_string(), "BM25".to_string()];

        let answer_lower = answer.to_lowercase();
        let missing: Vec<String> = should_mention
            .iter()
            .filter(|item| !answer_lower.contains(&item.to_lowercase()))
            .cloned()
            .collect();

        assert_eq!(missing, vec!["BM25".to_string()]);
    }

    #[test]
    fn test_eval_score_passes() {
        let score = EvalScore {
            relevance: 80,
            completeness: 75,
            accuracy: 90,
            coherence: 85,
            overall: 82,
        };
        assert!(score.passes(70));
        assert!(score.passes(82));
        assert!(!score.passes(83));
    }
}
