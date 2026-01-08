//! LLM-based answer evaluation
//!
//! Uses an LLM to subjectively evaluate answer quality against expected outcomes.

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
