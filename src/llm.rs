//! 狼人杀使用的 OpenAI-compatible JSON 对话客户端。

use anyhow::{Result, anyhow};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs, ReasoningEffort, ResponseFormat,
    },
};

pub struct LlmClient {
    client: Client<OpenAIConfig>,
    model: String,
    reasoning_effort: ReasoningEffort,
}

impl LlmClient {
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        let config = OpenAIConfig::new()
            .with_api_key(api_key)
            .with_api_base(base_url);
        Self {
            client: Client::with_config(config),
            model,
            reasoning_effort,
        }
    }

    pub async fn chat_json(&self, system: &str, user: &str) -> Result<String> {
        self.chat_json_with_messages(&[
            ("system".to_string(), system.to_string()),
            ("user".to_string(), user.to_string()),
        ])
        .await
    }

    pub async fn chat_json_with_messages(&self, msgs: &[(String, String)]) -> Result<String> {
        let to_message = |role: &str, content: &str| -> Result<ChatCompletionRequestMessage> {
            Ok(match role {
                "system" => ChatCompletionRequestSystemMessageArgs::default()
                    .content(content)
                    .build()?
                    .into(),
                "user" => ChatCompletionRequestUserMessageArgs::default()
                    .content(content)
                    .build()?
                    .into(),
                "assistant" => ChatCompletionRequestAssistantMessageArgs::default()
                    .content(content)
                    .build()?
                    .into(),
                other => return Err(anyhow!("unknown chat role: {other}")),
            })
        };

        let mut chain = msgs.to_vec();
        const MAX_ATTEMPTS: usize = 4;
        let mut last_reason = String::new();
        for attempt in 0..MAX_ATTEMPTS {
            let mut request_msgs = Vec::with_capacity(chain.len());
            for (role, content) in &chain {
                request_msgs.push(to_message(role, content)?);
            }
            let req = CreateChatCompletionRequestArgs::default()
                .model(&self.model)
                .messages(request_msgs)
                .response_format(ResponseFormat::JsonObject)
                .temperature(0.6_f32)
                .reasoning_effort(self.reasoning_effort.clone())
                .build()?;
            let response = self.client.chat().create(req).await?;
            let choice = response
                .choices
                .first()
                .ok_or_else(|| anyhow!("empty LLM response"))?;
            let content = choice
                .message
                .content
                .as_deref()
                .map(str::trim)
                .unwrap_or("");
            if !content.is_empty() {
                return Ok(content.to_string());
            }
            last_reason = choice
                .finish_reason
                .map(|r| format!("{r:?}"))
                .unwrap_or_else(|| "unknown".into());
            tracing::debug!(attempt = attempt + 1, reason = %last_reason, "LLM returned empty content");
            chain.push(("assistant".into(), String::new()));
            chain.push((
                "user".into(),
                "请返回一个非空的合法 JSON 对象，不要使用 markdown 代码块。".into(),
            ));
        }
        Err(anyhow!(
            "LLM returned empty content after {MAX_ATTEMPTS} attempts (last finish_reason={last_reason})"
        ))
    }
}
