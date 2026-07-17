use crate::detectors::Finding;
use crate::parser::ParsedFile;
use once_cell::sync::Lazy;

/// Detect packages used in imports but not declared in dependency files
pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    let dep_map = build_dep_map(parsed_files);

    for file in parsed_files {
        if is_dep_file(&file.path) {
            continue;
        }

        let registry = match file.language.as_str() {
            "javascript" | "typescript" | "tsx" | "jsx" => "npm",
            "python" => "pypi",
            "rust" => "crates.io",
            _ => continue,
        };

        let declared = match dep_map.get(registry) {
            Some(pkgs) => pkgs,
            None => continue,
        };

        for import in &file.imports {
            let package = crate::detectors::extract_package_name(&import.name);

            let package = match package {
                Some(p) => p,
                None => continue,
            };

            // Skip relative, built-in, and stdlib imports
            if package.starts_with('.')
                || package.starts_with('/')
                || crate::detectors::hallucinated_imports::is_builtin(&package, registry)
            {
                continue;
            }

            if !declared.contains(&package) {
                let codemod = match registry {
                    "npm" => Some(format!("npm install {}", package)),
                    "pypi" => Some(format!("pip install {}", package)),
                    "crates.io" => Some(format!("cargo add {}", package)),
                    _ => None,
                };
                findings.push(Finding {
                    detector: "phantom-deps".to_string(),
                    severity: "blocking".to_string(),
                    file: file.path.clone(),
                    line: import.line,
                    column: import.column,
                    message: format!(
                        "Package `{}` is used but not declared in your dependency file.",
                        package
                    ),
                    suggestion: Some(format!(
                        "Add `{}` to your package manager's dependency file.",
                        package
                    )),
                    evidence: Some(import.name.clone()),
                    codemod,
                });
            }
        }
    }

    findings
}

fn build_dep_map(files: &[ParsedFile]) -> std::collections::HashMap<String, Vec<String>> {
    let mut map = std::collections::HashMap::new();

    for file in files {
        let path = file.path.to_lowercase();

        // npm
        if path.ends_with("package.json") {
            let pkgs = extract_npm_deps(&file.raw_content);
            map.entry("npm".to_string()).or_insert_with(Vec::new).extend(pkgs);
        }

        // Python
        if path.ends_with("requirements.txt")
            || path.ends_with("pyproject.toml")
            || path.ends_with("setup.py")
            || path.ends_with("setup.cfg")
        {
            let pkgs = extract_python_deps(&file.raw_content);
            map.entry("pypi".to_string()).or_insert_with(Vec::new).extend(pkgs);
        }

        // Rust
        if path.ends_with("cargo.toml") {
            let pkgs = extract_cargo_deps(&file.raw_content);
            map.entry("crates.io".to_string()).or_insert_with(Vec::new).extend(pkgs);
        }
    }

    map
}

fn extract_npm_deps(content: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(deps) = json.get("dependencies").and_then(|d| d.as_object()) {
            pkgs.extend(deps.keys().cloned());
        }
        if let Some(dev_deps) = json.get("devDependencies").and_then(|d| d.as_object()) {
            pkgs.extend(dev_deps.keys().cloned());
        }
    }
    pkgs
}

fn extract_python_deps(content: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        // Handle: package==1.0, package>=1.0, package
        let pkg = line
            .split(&['=', '<', '>', '~', '!', ';', ' ', '\t'][..])
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if !pkg.is_empty() {
            pkgs.push(pkg);
        }
    }
    pkgs
}

fn extract_cargo_deps(content: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    if let Ok(toml) = content.parse::<toml::Table>() {
        if let Some(deps) = toml.get("dependencies").and_then(|d| d.as_table()) {
            pkgs.extend(deps.keys().cloned());
        }
        if let Some(dev_deps) = toml.get("dev-dependencies").and_then(|d| d.as_table()) {
            pkgs.extend(dev_deps.keys().cloned());
        }
        if let Some(build_deps) = toml.get("build-dependencies").and_then(|d| d.as_table()) {
            pkgs.extend(build_deps.keys().cloned());
        }
    }
    pkgs
}

static DEP_FILE_NAMES: Lazy<std::collections::HashSet<&'static str>> = Lazy::new(|| {
    let mut set = std::collections::HashSet::new();
    set.insert("package.json");
    set.insert("requirements.txt");
    set.insert("pyproject.toml");
    set.insert("cargo.toml");
    set.insert("setup.py");
    set.insert("setup.cfg");
    set.insert("go.mod");
    set.insert("gemfile");
    set.insert("gemfile.lock");
    set
});

fn is_dep_file(path: &str) -> bool {
    // O(1) HashSet lookup instead of linear OR chain
    let filename = path.rsplit('/').next().unwrap_or("").to_lowercase();
    DEP_FILE_NAMES.contains(filename.as_str())
}


