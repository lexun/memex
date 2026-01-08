//! Query decomposition and enhancement for Atlas
//!
//! Transforms natural language queries into effective search terms
//! using LLM-based keyword extraction. This addresses the BM25 search
//! limitation where queries like "what do we know about memex" fail
//! because BM25 requires all terms to match.

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
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
    /// Temporal filter extracted from query (optional)
    pub temporal_filter: Option<TemporalFilter>,
}

/// A temporal filter for date-based queries
#[derive(Debug, Clone)]
pub struct TemporalFilter {
    /// Start of the date range (inclusive)
    pub start: Option<DateTime<Utc>>,
    /// End of the date range (exclusive)
    pub end: Option<DateTime<Utc>>,
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
                temporal_filter: TemporalParser::parse(query),
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

        // Parse temporal expressions from the original query
        let temporal_filter = TemporalParser::parse(query);

        debug!(
            "Decomposed '{}' into keywords: {:?}, intent: {:?}, temporal: {:?}",
            query, keywords, intent, temporal_filter
        );

        Ok(DecomposedQuery {
            original: query.to_string(),
            keywords,
            search_text,
            intent,
            temporal_filter,
        })
    }
}

/// Parses temporal expressions from natural language queries
pub struct TemporalParser;

impl TemporalParser {
    /// Parse temporal expressions from a query string
    ///
    /// Supports expressions like:
    /// - "today", "yesterday"
    /// - "this week", "last week"
    /// - "this month", "last month"
    /// - "this year", "last year"
    /// - "last N days/weeks/months"
    /// - "before/after/since <date>"
    pub fn parse(query: &str) -> Option<TemporalFilter> {
        let lower = query.to_lowercase();
        let now = Local::now();
        let today = now.date_naive();

        // "today"
        if lower.contains("today") {
            let start = today.and_hms_opt(0, 0, 0)?;
            let end = (today + Duration::days(1)).and_hms_opt(0, 0, 0)?;
            return Some(TemporalFilter {
                start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
                end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
            });
        }

        // "yesterday"
        if lower.contains("yesterday") {
            let yesterday = today - Duration::days(1);
            let start = yesterday.and_hms_opt(0, 0, 0)?;
            let end = today.and_hms_opt(0, 0, 0)?;
            return Some(TemporalFilter {
                start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
                end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
            });
        }

        // "this week" (Monday to Sunday)
        if lower.contains("this week") {
            let days_since_monday = today.weekday().num_days_from_monday();
            let monday = today - Duration::days(days_since_monday as i64);
            let next_monday = monday + Duration::days(7);
            let start = monday.and_hms_opt(0, 0, 0)?;
            let end = next_monday.and_hms_opt(0, 0, 0)?;
            return Some(TemporalFilter {
                start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
                end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
            });
        }

        // "last week"
        if lower.contains("last week") {
            let days_since_monday = today.weekday().num_days_from_monday();
            let this_monday = today - Duration::days(days_since_monday as i64);
            let last_monday = this_monday - Duration::days(7);
            let start = last_monday.and_hms_opt(0, 0, 0)?;
            let end = this_monday.and_hms_opt(0, 0, 0)?;
            return Some(TemporalFilter {
                start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
                end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
            });
        }

        // "this month"
        if lower.contains("this month") {
            let first_of_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            let first_of_next = if today.month() == 12 {
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)?
            } else {
                NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1)?
            };
            let start = first_of_month.and_hms_opt(0, 0, 0)?;
            let end = first_of_next.and_hms_opt(0, 0, 0)?;
            return Some(TemporalFilter {
                start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
                end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
            });
        }

        // "last month"
        if lower.contains("last month") {
            let first_of_this = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            let first_of_last = if today.month() == 1 {
                NaiveDate::from_ymd_opt(today.year() - 1, 12, 1)?
            } else {
                NaiveDate::from_ymd_opt(today.year(), today.month() - 1, 1)?
            };
            let start = first_of_last.and_hms_opt(0, 0, 0)?;
            let end = first_of_this.and_hms_opt(0, 0, 0)?;
            return Some(TemporalFilter {
                start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
                end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
            });
        }

        // "last N days/weeks/months"
        if let Some(filter) = Self::parse_last_n(&lower, today) {
            return Some(filter);
        }

        // "since <date>" or "after <date>"
        if let Some(filter) = Self::parse_since_after(&lower, today) {
            return Some(filter);
        }

        // "before <date>"
        if let Some(filter) = Self::parse_before(&lower) {
            return Some(filter);
        }

        None
    }

    /// Parse "last N days/weeks/months" patterns
    fn parse_last_n(lower: &str, today: NaiveDate) -> Option<TemporalFilter> {
        use regex::Regex;

        // Match "last N days/weeks/months"
        let re = Regex::new(r"last\s+(\d+)\s+(day|week|month)s?").ok()?;
        let caps = re.captures(lower)?;

        let n: i64 = caps.get(1)?.as_str().parse().ok()?;
        let unit = caps.get(2)?.as_str();

        let start_date = match unit {
            "day" => today - Duration::days(n),
            "week" => today - Duration::weeks(n),
            "month" => {
                // Approximate months as 30 days
                today - Duration::days(n * 30)
            }
            _ => return None,
        };

        let start = start_date.and_hms_opt(0, 0, 0)?;
        let end = (today + Duration::days(1)).and_hms_opt(0, 0, 0)?;

        Some(TemporalFilter {
            start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
            end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
        })
    }

    /// Parse "since <date>" or "after <date>" patterns
    fn parse_since_after(lower: &str, _today: NaiveDate) -> Option<TemporalFilter> {
        use regex::Regex;

        // Match "since/after <date>"
        let re = Regex::new(r"(?:since|after)\s+(\w+(?:\s+\d{1,2})?(?:,?\s+\d{4})?)").ok()?;
        let caps = re.captures(lower)?;

        let date_str = caps.get(1)?.as_str();
        let date = Self::parse_date_string(date_str)?;

        let start = date.and_hms_opt(0, 0, 0)?;

        Some(TemporalFilter {
            start: Some(Local.from_local_datetime(&start).single()?.with_timezone(&Utc)),
            end: None,
        })
    }

    /// Parse "before <date>" patterns
    fn parse_before(lower: &str) -> Option<TemporalFilter> {
        use regex::Regex;

        let re = Regex::new(r"before\s+(\w+(?:\s+\d{1,2})?(?:,?\s+\d{4})?)").ok()?;
        let caps = re.captures(lower)?;

        let date_str = caps.get(1)?.as_str();
        let date = Self::parse_date_string(date_str)?;

        let end = date.and_hms_opt(0, 0, 0)?;

        Some(TemporalFilter {
            start: None,
            end: Some(Local.from_local_datetime(&end).single()?.with_timezone(&Utc)),
        })
    }

    /// Parse a date string like "January", "January 15", "January 15, 2024"
    fn parse_date_string(s: &str) -> Option<NaiveDate> {
        let now = Local::now();
        let current_year = now.year();

        // Try parsing month names
        let months = [
            ("january", 1), ("february", 2), ("march", 3), ("april", 4),
            ("may", 5), ("june", 6), ("july", 7), ("august", 8),
            ("september", 9), ("october", 10), ("november", 11), ("december", 12),
            ("jan", 1), ("feb", 2), ("mar", 3), ("apr", 4),
            ("jun", 6), ("jul", 7), ("aug", 8), ("sep", 9),
            ("oct", 10), ("nov", 11), ("dec", 12),
        ];

        let lower = s.to_lowercase();
        let parts: Vec<&str> = lower.split_whitespace().collect();

        for (name, month) in months {
            if parts.first().map(|p| p.starts_with(name)).unwrap_or(false) {
                let day = if parts.len() > 1 {
                    parts[1].trim_end_matches(',').parse::<u32>().unwrap_or(1)
                } else {
                    1
                };

                let year = if parts.len() > 2 {
                    parts[2].parse::<i32>().unwrap_or(current_year)
                } else {
                    current_year
                };

                return NaiveDate::from_ymd_opt(year, month, day);
            }
        }

        None
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
5. IMPORTANT: Include synonyms for ambiguous terms (apps/projects, setup/configuration, etc.)
6. Identify the query intent type

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
- "what apps am I working on" -> {"keywords": ["apps", "projects", "working"], "intent": "enumerative"}

Keep 1-5 keywords. Prefer fewer, more specific terms over many generic ones.
For technical domains, use common abbreviations (DEV, PROD, DB, API, CLI, etc.).
Include synonyms when the term could have alternatives (apps→projects, setup→configuration)."#;

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

    #[test]
    fn test_temporal_parser_today() {
        let filter = TemporalParser::parse("what did we work on today").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_some());
        // Start should be earlier than end
        assert!(filter.start.unwrap() < filter.end.unwrap());
    }

    #[test]
    fn test_temporal_parser_yesterday() {
        let filter = TemporalParser::parse("changes from yesterday").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_some());
        assert!(filter.start.unwrap() < filter.end.unwrap());
    }

    #[test]
    fn test_temporal_parser_this_week() {
        let filter = TemporalParser::parse("progress this week").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_some());
    }

    #[test]
    fn test_temporal_parser_last_week() {
        let filter = TemporalParser::parse("what happened last week").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_some());
    }

    #[test]
    fn test_temporal_parser_this_month() {
        let filter = TemporalParser::parse("tasks this month").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_some());
    }

    #[test]
    fn test_temporal_parser_last_n_days() {
        let filter = TemporalParser::parse("last 7 days").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_some());
    }

    #[test]
    fn test_temporal_parser_since_month() {
        let filter = TemporalParser::parse("changes since January").unwrap();
        assert!(filter.start.is_some());
        assert!(filter.end.is_none()); // Open-ended
    }

    #[test]
    fn test_temporal_parser_no_temporal() {
        let filter = TemporalParser::parse("what is memex");
        assert!(filter.is_none());
    }
}
