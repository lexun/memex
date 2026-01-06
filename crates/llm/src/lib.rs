//! LLM client abstraction for memex
//!
//! Provides a unified interface for LLM providers (OpenAI, Anthropic, Ollama).

use anyhow::{Context, Result};
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
        CreateEmbeddingRequestArgs, EmbeddingInput,
    },
    Client as OpenAIClient,
};
use serde::{Deserialize, Serialize};

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider name (openai, anthropic, ollama)
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Model to use for chat completions
    #[serde(default = "default_model")]
    pub model: String,
    /// Model to use for embeddings
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    /// API key (optional if using env var or local provider)
    pub api_key: Option<String>,
    /// Base URL override (for custom endpoints)
    pub base_url: Option<String>,
}

fn default_provider() -> String {
    "openai".to_string()
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            embedding_model: default_embedding_model(),
            api_key: None,
            base_url: None,
        }
    }
}

/// A message in a chat conversation
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// LLM client abstraction
pub struct LlmClient {
    config: LlmConfig,
}

impl LlmClient {
    /// Create a new LLM client with the given configuration
    pub fn new(config: LlmConfig) -> Self {
        Self { config }
    }

    /// Generate a chat completion
    pub async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        match self.config.provider.as_str() {
            "openai" => self.chat_openai(messages).await,
            provider => anyhow::bail!("Unsupported LLM provider: {}", provider),
        }
    }

    /// Simple completion with a system prompt and user message
    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.chat(vec![Message::system(system), Message::user(user)])
            .await
    }

    /// Generate embeddings for a list of texts
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        match self.config.provider.as_str() {
            "openai" => self.embed_openai(texts).await,
            provider => anyhow::bail!("Unsupported embedding provider: {}", provider),
        }
    }

    /// Generate embedding for a single text
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed(vec![text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .context("No embedding returned")
    }

    async fn embed_openai(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let mut openai_config = OpenAIConfig::new();

        if let Some(api_key) = &self.config.api_key {
            openai_config = openai_config.with_api_key(api_key);
        }

        if let Some(base_url) = &self.config.base_url {
            openai_config = openai_config.with_api_base(base_url);
        }

        let client = OpenAIClient::with_config(openai_config);

        let request = CreateEmbeddingRequestArgs::default()
            .model(&self.config.embedding_model)
            .input(EmbeddingInput::StringArray(texts))
            .build()
            .context("Failed to build embedding request")?;

        let response = client
            .embeddings()
            .create(request)
            .await
            .context("Failed to create embeddings")?;

        let embeddings = response
            .data
            .into_iter()
            .map(|e| e.embedding)
            .collect();

        Ok(embeddings)
    }

    async fn chat_openai(&self, messages: Vec<Message>) -> Result<String> {
        let mut openai_config = OpenAIConfig::new();

        if let Some(api_key) = &self.config.api_key {
            openai_config = openai_config.with_api_key(api_key);
        }

        if let Some(base_url) = &self.config.base_url {
            openai_config = openai_config.with_api_base(base_url);
        }

        let client = OpenAIClient::with_config(openai_config);

        let openai_messages: Vec<ChatCompletionRequestMessage> = messages
            .into_iter()
            .map(|msg| match msg.role {
                Role::System => ChatCompletionRequestSystemMessageArgs::default()
                    .content(msg.content)
                    .build()
                    .unwrap()
                    .into(),
                Role::User => ChatCompletionRequestUserMessageArgs::default()
                    .content(msg.content)
                    .build()
                    .unwrap()
                    .into(),
                Role::Assistant => {
                    use async_openai::types::ChatCompletionRequestAssistantMessageArgs;
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .content(msg.content)
                        .build()
                        .unwrap()
                        .into()
                }
            })
            .collect();

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.config.model)
            .messages(openai_messages)
            .build()
            .context("Failed to build chat completion request")?;

        let response = client
            .chat()
            .create(request)
            .await
            .context("Failed to create chat completion")?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(content)
    }

    /// Get the configured model name
    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Get the configured provider name
    pub fn provider(&self) -> &str {
        &self.config.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LlmConfig::default();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4o-mini");
    }

    #[test]
    fn test_message_builders() {
        let sys = Message::system("You are helpful");
        assert_eq!(sys.role, Role::System);
        assert_eq!(sys.content, "You are helpful");

        let user = Message::user("Hello");
        assert_eq!(user.role, Role::User);
        assert_eq!(user.content, "Hello");
    }
}
