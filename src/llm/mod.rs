use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default, skip_serializing)]
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
    // Force free models in dev/test. Set ENVIRONMENT=Prod to use paid models.
    let env = std::env::var("ENVIRONMENT").unwrap_or_default();
    if env.eq_ignore_ascii_case("prod") {
        "anthropic/claude-sonnet-4.6".to_string()
    } else {
        "qwen/qwen3-coder:free".to_string()
    }
}

fn default_max_tokens() -> u32 {
    2048
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

/// Context about the review being performed
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewContext {
    /// Repository name (e.g. "owner/repo")
    pub repo: Option<String>,

    /// Branch or ref being reviewed
    pub branch: Option<String>,

    /// PR title if reviewing a pull request
    pub pr_title: Option<String>,

    /// PR description / body
    pub pr_description: Option<String>,

    /// Linked issue numbers and their content
    pub linked_issues: Vec<IssueContext>,

    /// Related PRs that touched the same areas
    pub related_prs: Vec<String>,

    /// Repository codebase context (files, languages, dependencies)
    pub repo_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueContext {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
}

impl fmt::Display for ReviewContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(repo) = &self.repo {
            writeln!(f, "Repository: {}", repo)?;
        }
        if let Some(branch) = &self.branch {
            writeln!(f, "Branch: {}", branch)?;
        }
        if let Some(title) = &self.pr_title {
            writeln!(f, "PR Title: {}", title)?;
        }
        if let Some(body) = &self.pr_description {
            writeln!(f, "PR Description: {}", body)?;
        }
        if !self.linked_issues.is_empty() {
            writeln!(f, "\nLinked Issues:")?;
            for issue in &self.linked_issues {
                writeln!(f, "  #{}: {}", issue.number, issue.title)?;
                if let Some(body) = &issue.body {
                    let preview: String = body.chars().take(200).collect();
                    writeln!(f, "    {}", preview)?;
                }
            }
        }
        if !self.related_prs.is_empty() {
            writeln!(f, "\nRelated PRs: {}", self.related_prs.join(", "))?;
        }
        if let Some(ctx) = &self.repo_context {
            writeln!(f, "\n{}", ctx)?;
        }
        Ok(())
    }
}

/// Reviews a diff with optional PR/issue context.
pub async fn review_diff(
    diff: &str,
    config: &LlmConfig,
    context: Option<&ReviewContext>,
) -> Result<LlmReviewOutput> {
    let prompt = build_review_prompt(diff, context);

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

    let system_prompt = "\
You are a senior staff engineer reviewing a pull request. You have 15+ years of experience \
shipping production systems and have seen every category of bug, anti-pattern, and design \
mistake. You are direct, precise, and opinionated.

HOW YOU REVIEW:
- You read the diff with a focus on what will break in production, not style or nitpicks.
- You prioritize issues by impact: security holes > correctness bugs > performance > maintainability.
- You ignore whitespace, import ordering, naming conventions, and other bikeshed topics.
- You are comfortable saying \"no issues found\" when the code is solid.

YOUR OUTPUT:
- For each issue: file:line, severity (critical/warning/info), category, description, and a \
concrete suggested fix (<=30 words).
- Your verdict is one of: \"ship\" (merge as-is), \"fix-before-ship\" (address issues first), \
or \"hold\" (needs design discussion).
- If you are unsure about a finding, set confidence to \"low\" and explain why. \
Never report something you made up.

RULES:
1. Only report issues you are confident are real. False positives erode trust.
2. No evidence = no finding. Every issue must cite the specific file and line.
3. If the PR description references requirements, verify the code actually implements them.
4. Consider: null safety, error handling, concurrency, input validation, authz, data leakage.
5. If the diff is large, focus on the most impactful changes, not the first ones you see.";

    let body = json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": system_prompt
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
        bail!("LLM API returned {}: {}", status, error_text);
    }

    let resp_json: serde_json::Value = resp.json().await?;
    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .context("LLM response missing content")?;

    let output: LlmReviewOutput =
        serde_json::from_str(content).context("Failed to parse LLM response")?;

    Ok(output)
}

/// Builds a prompt for reviewing a code diff, with optional PR/issue context.
pub fn build_review_prompt(diff: &str, context: Option<&ReviewContext>) -> String {
    const MAX_DIFF_LENGTH: usize = 8000;

    let truncated = if diff.len() > MAX_DIFF_LENGTH {
        // Find the nearest char boundary to avoid mid-character panic on multi-byte UTF-8
        let trunc_byte = MAX_DIFF_LENGTH.min(diff.len());
        let trunc_byte = if diff.is_char_boundary(trunc_byte) {
            trunc_byte
        } else {
            diff.char_indices()
                .take_while(|&(i, _)| i < trunc_byte)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(trunc_byte)
        };
        format!(
            "{}\n\n[Diff truncated: original was {} characters, showing first {}]",
            &diff[..trunc_byte],
            diff.len(),
            MAX_DIFF_LENGTH
        )
    } else {
        diff.to_string()
    };

    let context_section = match context {
        Some(ctx) => {
            let ctx_str = ctx.to_string();
            if ctx_str.trim().is_empty() {
                String::new()
            } else {
                format!("---\n\nContext:\n{}\n", ctx_str)
            }
        }
        None => String::new(),
    };

    if context_section.is_empty() {
        format!(
            r#"Review the diff below.

Focus on security, logic bugs, API misuse, and edge cases.
Only report issues you are highly confident about.

Diff:
```
{}
```"#,
            truncated
        )
    } else {
        format!(
            r#"{}Review the diff below.

Focus on security, logic bugs, API misuse, and edge cases.
Only report issues you are highly confident about.

Diff:
```
{}
```"#,
            context_section, truncated
        )
    }
}
