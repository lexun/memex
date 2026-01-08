//! Scenario definitions for evaluation
//!
//! Defines the structure of test scenarios including actions to perform
//! and queries to evaluate.

use serde::{Deserialize, Serialize};

/// A complete test scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Unique name for the scenario
    pub name: String,
    /// Description of what this scenario tests
    pub description: String,
    /// Actions to perform to set up the scenario
    pub setup: Vec<Action>,
    /// Queries to run and evaluate
    pub queries: Vec<Query>,
}

/// An action to perform during scenario setup
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    /// Record a memo
    RecordMemo {
        content: String,
        /// Optional delay in seconds before this action (simulates time passing)
        #[serde(default)]
        delay_secs: u64,
    },
    /// Create a task
    CreateTask {
        title: String,
        description: Option<String>,
        project: Option<String>,
    },
    /// Add a note to a task (references task by title from this scenario)
    AddTaskNote { task_title: String, content: String },
    /// Close a task
    CloseTask {
        task_title: String,
        reason: Option<String>,
    },
}

/// A query to evaluate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    /// The query text
    pub query: String,
    /// What we expect the answer to contain/address
    pub expected: ExpectedOutcome,
}

/// Expected outcome for evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutcome {
    /// Key facts the answer should mention
    pub should_mention: Vec<String>,
    /// Key facts the answer should NOT mention (to test filtering)
    #[serde(default)]
    pub should_not_mention: Vec<String>,
    /// The type of answer expected
    #[serde(default)]
    pub answer_type: AnswerType,
}

/// Type of answer expected
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum AnswerType {
    /// A factual response with specific information
    #[default]
    Factual,
    /// A summary of multiple items
    Summary,
    /// A list of items
    List,
    /// An explanation of a concept
    Explanation,
}

impl Scenario {
    /// Create a new scenario
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            setup: Vec::new(),
            queries: Vec::new(),
        }
    }

    /// Add a memo recording action
    pub fn record_memo(mut self, content: impl Into<String>) -> Self {
        self.setup.push(Action::RecordMemo {
            content: content.into(),
            delay_secs: 0,
        });
        self
    }

    /// Add a memo recording action with delay
    pub fn record_memo_delayed(mut self, content: impl Into<String>, delay_secs: u64) -> Self {
        self.setup.push(Action::RecordMemo {
            content: content.into(),
            delay_secs,
        });
        self
    }

    /// Add a task creation action
    pub fn create_task(
        mut self,
        title: impl Into<String>,
        description: Option<String>,
        project: Option<String>,
    ) -> Self {
        self.setup.push(Action::CreateTask {
            title: title.into(),
            description,
            project,
        });
        self
    }

    /// Add a query to evaluate
    pub fn query(mut self, query: impl Into<String>, expected: ExpectedOutcome) -> Self {
        self.queries.push(Query {
            query: query.into(),
            expected,
        });
        self
    }
}

/// Built-in test scenarios
pub fn builtin_scenarios() -> Vec<Scenario> {
    vec![
        // Scenario 1: Basic memo retrieval
        Scenario::new(
            "basic_memo_retrieval",
            "Tests basic memo recording and retrieval",
        )
        .record_memo("The memex project uses SurrealDB as its database backend")
        .record_memo("We decided to use BM25 combined with vector search for hybrid retrieval")
        .query(
            "What database does memex use?",
            ExpectedOutcome {
                should_mention: vec!["SurrealDB".to_string()],
                should_not_mention: vec![],
                answer_type: AnswerType::Factual,
            },
        )
        .query(
            "How does search work in memex?",
            ExpectedOutcome {
                should_mention: vec!["BM25".to_string(), "vector".to_string()],
                should_not_mention: vec![],
                answer_type: AnswerType::Explanation,
            },
        ),

        // Scenario 2: Task status tracking
        Scenario::new(
            "task_status_tracking",
            "Tests task creation and status queries",
        )
        .create_task(
            "Implement user authentication",
            Some("Add OAuth2 login flow".to_string()),
            Some("memex".to_string()),
        )
        .create_task(
            "Add rate limiting",
            Some("Prevent API abuse".to_string()),
            Some("memex".to_string()),
        )
        .query(
            "What tasks are we working on for memex?",
            ExpectedOutcome {
                should_mention: vec![
                    "authentication".to_string(),
                    "rate limiting".to_string(),
                ],
                should_not_mention: vec![],
                answer_type: AnswerType::List,
            },
        ),

        // Scenario 3: Decision tracking
        Scenario::new(
            "decision_tracking",
            "Tests recording and retrieving decisions",
        )
        .record_memo("DECISION: We will use Rust for the backend due to its performance and safety guarantees")
        .record_memo("DECISION: The API will use JSON-RPC over Unix sockets for local IPC")
        .record_memo("We considered using gRPC but decided against it due to complexity")
        .query(
            "What decisions have we made about the API?",
            ExpectedOutcome {
                should_mention: vec!["JSON-RPC".to_string(), "Unix sockets".to_string()],
                should_not_mention: vec![],
                answer_type: AnswerType::Summary,
            },
        ),

        // Scenario 4: Technical context
        Scenario::new(
            "technical_context",
            "Tests technical knowledge retrieval",
        )
        .record_memo("The daemon uses a fork+exec pattern for backgrounding. It forks, the parent exits, and the child exec's itself with a --run flag")
        .record_memo("Knowledge extraction uses gpt-4o-mini for cost efficiency. Facts are extracted from memos and linked to entities")
        .query(
            "How does the daemon start in the background?",
            ExpectedOutcome {
                should_mention: vec!["fork".to_string(), "exec".to_string()],
                should_not_mention: vec![],
                answer_type: AnswerType::Explanation,
            },
        ),
    ]
}
