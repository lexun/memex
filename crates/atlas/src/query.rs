//! Query decomposition and enhancement for Atlas
//!
//! Transforms natural language queries into effective search terms
//! using LLM-based keyword extraction. This addresses the BM25 search
//! limitation where queries like "what do we know about memex" fail
//! because BM25 requires all terms to match.

use anyhow::{Context, Result};
use llm::LlmClient;
use serde::Deserialize;
use tracing::debug;

/// Result of decomposing a query
#[derive(Debug, Clone)]
pub struct DecomposedQuery {
    /// Original user query
    pub original: String,
    /// Extracted keywords for BM25 search
    pub keywords: Vec<String>,
    /// Combined search terms (space-joined keywords)
    pub search_text: String,
    /// Query type/intent
    pub intent: QueryIntent,
}

/// The type/intent of a query
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum QueryIntent {
    /// Factual query: "what is X", "tell me about X"
    #[default]
    Factual,
    /// Relational query: "how does X relate to Y"
    Relational,
    /// Procedural query: "how do I X", "steps to X"
    Procedural,
    /// Temporal query: "when did X happen"
    Temporal,
    /// Enumerative query: "list all X", "what are the X"
    Enumerative,
}

/// Raw decomposition response from LLM
#[derive(Debug, Deserialize)]
struct RawDecomposition {
    /// Key terms extracted from the query
    #[serde(default)]
    keywords: Vec<String>,
    /// Query intent type
    #[serde(default)]
    intent: String,
}

/// Query decomposer using LLM
pub struct QueryDecomposer<'a> {
    llm: &'a LlmClient,
}

impl<'a> QueryDecomposer<'a> {
    /// Create a new decomposer with the given LLM client
    pub fn new(llm: &'a LlmClient) -> Self {
        Self { llm }
    }

    /// Decompose a natural language query into effective search terms
    ///
    /// This extracts keywords, entities, and technical terms while
    /// removing stop words and filler. The result can be used for
    /// both BM25 text search and as context for embeddings.
    pub async fn decompose(&self, query: &str) -> Result<DecomposedQuery> {
        // Skip decomposition for very short queries (likely already keywords)
        if query.split_whitespace().count() <= 2 {
            debug!("Query '{}' is short, skipping decomposition", query);
            return Ok(DecomposedQuery {
                original: query.to_string(),
                keywords: vec![query.to_string()],
                search_text: query.to_string(),
                intent: QueryIntent::Factual,
            });
        }

        let response = self
            .llm
            .complete(DECOMPOSITION_PROMPT, query)
            .await
            .context("LLM decomposition failed")?;

        debug!("Raw decomposition response: {}", response);

        // Parse JSON from response (may be wrapped in markdown code block)
        let json_str = extract_json(&response);
        let raw: RawDecomposition = serde_json::from_str(json_str)
            .with_context(|| format!("Failed to parse decomposition response: {}", json_str))?;

        // Ensure we have at least some keywords
        let keywords = if raw.keywords.is_empty() {
            // Fallback: use original query
            vec![query.to_string()]
        } else {
            raw.keywords
        };

        let search_text = keywords.join(" ");
        let intent = parse_intent(&raw.intent);

        debug!(
            "Decomposed '{}' into keywords: {:?}, intent: {:?}",
            query, keywords, intent
        );

        Ok(DecomposedQuery {
            original: query.to_string(),
            keywords,
            search_text,
            intent,
        })
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

fn parse_intent(s: &str) -> QueryIntent {
    match s.to_lowercase().as_str() {
        "relational" | "relationship" | "relation" => QueryIntent::Relational,
        "procedural" | "procedure" | "how-to" | "howto" => QueryIntent::Procedural,
        "temporal" | "time" | "when" => QueryIntent::Temporal,
        "enumerative" | "list" | "enumerate" => QueryIntent::Enumerative,
        _ => QueryIntent::Factual,
    }
}

const DECOMPOSITION_PROMPT: &str = r#"You are a search query optimizer. Extract the key search terms from the user's natural language query.

Your task:
1. Identify the important nouns, entities, and technical terms
2. Remove stop words (what, is, the, how, do, we, know, about, etc.)
3. Keep proper nouns, project names, technical terms, and key concepts
4. IMPORTANT: Include common abbreviations alongside full terms (dev/DEV, prod/PROD, config, etc.)
5. Identify the query intent type

Respond with JSON only (no markdown, no explanation):
{
  "keywords": ["term1", "term2", "term3"],
  "intent": "factual|relational|procedural|temporal|enumerative"
}

Intent types:
- factual: asking what something is or about its properties
- relational: asking how things relate or connect
- procedural: asking how to do something
- temporal: asking about when something happened
- enumerative: asking for a list of things

Examples:
- "what do we know about memex" -> {"keywords": ["memex"], "intent": "factual"}
- "how does the daemon connect to surrealdb" -> {"keywords": ["daemon", "surrealdb"], "intent": "relational"}
- "how do I configure the LLM provider" -> {"keywords": ["configure", "LLM", "provider"], "intent": "procedural"}
- "when was the fact extraction added" -> {"keywords": ["fact", "extraction"], "intent": "temporal"}
- "list all the MCP tools" -> {"keywords": ["MCP", "tools"], "intent": "enumerative"}
- "development environment setup" -> {"keywords": ["DEV", "environment"], "intent": "procedural"}
- "what is the difference between dev and prod" -> {"keywords": ["DEV", "PROD"], "intent": "factual"}

Keep 1-5 keywords. Prefer fewer, more specific terms over many generic ones.
For technical domains, use common abbreviations (DEV, PROD, DB, API, CLI, etc.)."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"keywords": ["test"], "intent": "factual"}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_code_block() {
        let input = r#"```json
{"keywords": ["test"], "intent": "factual"}
```"#;
        assert_eq!(
            extract_json(input),
            r#"{"keywords": ["test"], "intent": "factual"}"#
        );
    }

    #[test]
    fn test_parse_intent() {
        assert_eq!(parse_intent("factual"), QueryIntent::Factual);
        assert_eq!(parse_intent("relational"), QueryIntent::Relational);
        assert_eq!(parse_intent("PROCEDURAL"), QueryIntent::Procedural);
        assert_eq!(parse_intent("temporal"), QueryIntent::Temporal);
        assert_eq!(parse_intent("list"), QueryIntent::Enumerative);
        assert_eq!(parse_intent("unknown"), QueryIntent::Factual);
    }
}
