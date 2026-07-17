use anyhow::Result;
use crate::bot::WebhookPayload;

pub async fn review_pr(token: &str, payload: &WebhookPayload) -> Result<()> {
    let pr = match &payload.pull_request {
        Some(p) => p,
        None => return Ok(()),
    };

    let repo_name = pr["head"]["repo"]["full_name"]
        .as_str()
        .unwrap_or("unknown");
    let pr_number = pr["number"].as_i64().unwrap_or(0);

    let client = reqwest::Client::new();
    let auth_header = format!("Bearer {}", token);

    let diff_text: String = client
        .get(format!(
            "https://api.github.com/repos/{}/pulls/{}/files",
            repo_name, pr_number
        ))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .send()
        .await?
        .text()
        .await?;

    let files: Vec<serde_json::Value> = serde_json::from_str(&diff_text).unwrap_or_default();
    if files.is_empty() {
        return Ok(());
    }

    let mut findings = crate::detectors::Findings::new();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        let patch = file["patch"].as_str().unwrap_or("");
        if !patch.is_empty() && patch.len() < 100_000 {
            let parsed = crate::parser::parse_file(filename, patch).ok();
            if let Some(p) = parsed {
                findings.extend(crate::detectors::run_all(&[p], &crate::config::Config::default()).findings);
            }
        }
    }

    if findings.is_empty() {
        return Ok(());
    }

    let mut body = String::from("## 🦕 Codasaurus Review\n\n");
    let counts = findings.count_by_severity();
    body.push_str(&format!(
        "Found **{}** issue(s): {} blocking, {} warnings\n\n",
        findings.findings.len(),
        counts.get("blocking").unwrap_or(&0),
        counts.get("warning").unwrap_or(&0),
    ));

    for f in &findings.findings {
        let icon = match f.severity.as_str() {
            "blocking" => "🔴",
            "warning" => "🟡",
            _ => "🔵",
        };
        body.push_str(&format!(
            "{} **{}** `{}:{}` — {}",
            icon, f.severity, f.file, f.line, f.message
        ));
        if let Some(s) = &f.suggestion {
            body.push_str(&format!(" _{}_{}", "", s));
        }
        body.push('\n');
    }

    let comment = serde_json::json!({"body": body});
    let _: serde_json::Value = client
        .post(format!(
            "https://api.github.com/repos/{}/issues/{}/comments",
            repo_name, pr_number
        ))
        .header("Authorization", &auth_header)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "codasaurus/0.1.0")
        .json(&comment)
        .send()
        .await?
        .json()
        .await?;

    Ok(())
}
