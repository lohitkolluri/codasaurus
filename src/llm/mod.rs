use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub api_key: String,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_base_url")]
    pub base_url: String,
}

fn default_model() -> String {
    "anthropic/claude-sonnet-4.6".to_string()
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_temperature() -> f32 {
    0.1
}

fn default_base_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}

impl LlmConfig {
    /// Reads CODASAURUS_API_KEY first, then OPENROUTER_API_KEY as fallback.
    /// Returns `None` if neither env var is set.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("CODASAURUS_API_KEY")
            .or_else(|_| std::env::var("OPENROUTER_API_KEY"))
            .ok()?;

        if api_key.is_empty() {
            return None;
        }

        Some(Self {
            api_key,
            model: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            base_url: default_base_url(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmReviewOutput {
    pub verdict: String,
    pub issues: Vec<LlmIssue>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmIssue {
    pub severity: String,
    pub category: String,
    pub file: String,
    #[serde(default)]
    pub line: usize,
    pub description: String,
    pub suggestion: Option<String>,
    #[serde(default = "default_confidence")]
    pub confidence: String,
}

fn default_confidence() -> String {
    "medium".to_string()
}

/// Returns a JSON Schema value that matches the LlmReviewOutput structure.
pub fn review_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "verdict": {
                "type": "string",
                "description": "Overall verdict of the review"
            },
            "issues": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "severity": {
                            "type": "string",
                            "enum": ["critical", "warning", "info"],
                            "description": "How severe the issue is"
                        },
                        "category": {
                            "type": "string",
                            "enum": ["security", "logic", "performance", "maintainability", "correctness"],
                            "description": "Category of the issue"
                        },
                        "file": {
                            "type": "string",
                            "description": "File where the issue was found"
                        },
                        "line": {
                            "type": "integer",
                            "description": "Line number of the issue"
                        },
                        "description": {
                            "type": "string",
                            "description": "Description of the issue"
                        },
                        "suggestion": {
                            "type": "string",
                            "description": "Suggested fix for the issue"
                        },
                        "confidence": {
                            "type": "string",
                            "enum": ["high", "medium", "low"],
                            "description": "Confidence in this finding"
                        }
                    },
                    "required": [
                        "severity",
                        "category",
                        "file",
                        "description",
                        "confidence"
                    ]
                },
                "description": "List of issues found in the review"
            },
            "summary": {
                "type": "string",
                "description": "Optional summary of the review"
            }
        },
        "required": ["verdict", "issues"]
    })
}

/// Reviews a diff by sending it to an LLM and parsing the structured response.
pub async fn review_diff(diff: &str, config: &LlmConfig) -> Result<LlmReviewOutput> {
    let prompt = build_review_prompt(diff);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let schema = review_schema();
    let response_format = json!({
        "type": "json_schema",
        "json_schema": {
            "name": "review_response",
            "strict": true,
            "schema": schema
        }
    });

    let body = json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": "You are a code review assistant. Review the provided diff and respond with a JSON object matching the specified schema."
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "response_format": response_format,
        "max_tokens": config.max_tokens,
        "temperature": config.temperature
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://github.com/lohitkolluri/codasaurus")
        .header("X-Title", "Codasaurus")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let error_text = resp.text().await.unwrap_or_default();
        bail!(
            "LLM API returned {}: {}",
            status,
            error_text
        );
    }

    let resp_json: serde_json::Value = resp.json().await?;
    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("LLM response missing content")?;

    let output: LlmReviewOutput =
        serde_json::from_str(content).context("Failed to parse LLM response as LlmReviewOutput")?;

    Ok(output)
}

/// Builds a prompt for reviewing a code diff.
pub fn build_review_prompt(diff: &str) -> String {
    const MAX_DIFF_LENGTH: usize = 8000;

    let truncated = if diff.len() > MAX_DIFF_LENGTH {
        format!(
            "{}\n\n[Diff truncated: original was {} characters, showing first {}]",
            &diff[..MAX_DIFF_LENGTH],
            diff.len(),
            MAX_DIFF_LENGTH
        )
    } else {
        diff.to_string()
    };

    format!(
        r#"Review the following code diff and identify potential issues.

Focus on these areas:
- **Security**: Vulnerabilities, injection risks, unsafe handling of inputs
- **Logic bugs**: Off-by-one, incorrect conditions, missing edge cases
- **API misuse**: Incorrect function signatures, missing error handling
- **Maintainability**: Dead code, excessive complexity, poor naming
- **Edge cases**: Null/unexpected inputs, boundary conditions, concurrency issues

IMPORTANT: Only report issues you are highly confident about. False positives are worse than missing a real issue.

Diff:
```
{}
```"#,
        truncated
    )
}
