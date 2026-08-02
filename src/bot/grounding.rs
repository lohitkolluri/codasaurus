//! Lightweight LLM grounding: path neighbors + symbol hints (no embeddings).

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

static SYMBOL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\+?\s*(?:(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|enum|trait|type|impl)\s+(\w+)|(?:export\s+)?(?:async\s+)?(?:function|class|const|let|type|interface)\s+(\w+)|def\s+(\w+)|func\s+(\w+))",
    )
    .expect("symbol regex")
});

const MAX_SYMBOLS: usize = 24;
const MAX_NEIGHBORS: usize = 8;
const MAX_GROUNDING_CHARS: usize = 2_500;

/// Build a short grounding block from changed paths + patch text.
pub fn build_grounding_block(changed_paths: &[String], patches: &[(String, String)]) -> String {
    let mut symbols: BTreeSet<String> = BTreeSet::new();
    for (_path, patch) in patches {
        for cap in SYMBOL_RE.captures_iter(patch) {
            for i in 1..=4 {
                if let Some(m) = cap.get(i) {
                    let name = m.as_str();
                    if name.len() >= 2 && name != "self" {
                        symbols.insert(name.to_string());
                    }
                }
            }
            if symbols.len() >= MAX_SYMBOLS {
                break;
            }
        }
        if symbols.len() >= MAX_SYMBOLS {
            break;
        }
    }

    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let path_set: BTreeSet<&str> = changed_paths.iter().map(|s| s.as_str()).collect();
    for p in changed_paths {
        if let Some((dir, _)) = p.rsplit_once('/') {
            if !dir.is_empty() {
                dirs.insert(dir.to_string());
            }
        }
    }

    // Sibling paths already in the PR (same directory) — no extra GitHub fetch required.
    let mut neighbors: Vec<String> = Vec::new();
    for p in changed_paths {
        let Some((dir, _)) = p.rsplit_once('/') else {
            continue;
        };
        for other in &path_set {
            if *other == p.as_str() {
                continue;
            }
            if other.rsplit_once('/').map(|(d, _)| d) == Some(dir) {
                neighbors.push((*other).to_string());
            }
        }
    }
    neighbors.sort();
    neighbors.dedup();
    neighbors.truncate(MAX_NEIGHBORS);

    if symbols.is_empty() && neighbors.is_empty() && dirs.is_empty() {
        return String::new();
    }

    let mut out = String::from("## Local grounding (deterministic)\n");
    if !dirs.is_empty() {
        out.push_str("Changed directories: ");
        out.push_str(
            &dirs
                .iter()
                .take(6)
                .map(|d| format!("`{d}`"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    if !symbols.is_empty() {
        out.push_str("Symbols touched in the diff: ");
        out.push_str(
            &symbols
                .iter()
                .take(MAX_SYMBOLS)
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    if !neighbors.is_empty() {
        out.push_str("Sibling files in this PR (same directory):\n");
        for n in &neighbors {
            out.push_str(&format!("- `{n}`\n"));
        }
    }
    out.push_str(
        "Prefer citing these symbols/paths when relevant. Do not invent files not listed.\n",
    );

    if out.len() > MAX_GROUNDING_CHARS {
        out.truncate(MAX_GROUNDING_CHARS);
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_and_ts_symbols() {
        let patch = "\
@@ -1,3 +1,8 @@
+pub fn apply_title_fix() {}\n\
+export async function reviewDiff() {}\n\
+def charge_customer():\n\
";
        let block = build_grounding_block(
            &["src/bot/title_fix.rs".into(), "src/bot/mod.rs".into()],
            &[("src/bot/title_fix.rs".into(), patch.into())],
        );
        assert!(block.contains("apply_title_fix") || block.contains("Symbols"));
        assert!(block.contains("Sibling") || block.contains("src/bot/mod.rs"));
        assert!(block.contains("Local grounding"));
    }

    #[test]
    fn empty_without_signal() {
        assert!(build_grounding_block(&[], &[]).is_empty());
    }
}
