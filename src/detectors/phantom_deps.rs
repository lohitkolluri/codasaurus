use crate::detectors::Finding;
use crate::parser::ParsedFile;

pub fn detect(parsed_files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();

    let dep_map = build_dep_map(parsed_files);

    for file in parsed_files {
        // Skip dependency manifest files themselves
        let fname = file.path.rsplit('/').next().unwrap_or("").to_lowercase();
        if matches!(
            fname.as_str(),
            "package.json"
                | "requirements.txt"
                | "pyproject.toml"
                | "cargo.toml"
                | "setup.py"
                | "setup.cfg"
                | "go.mod"
                | "gemfile"
                | "gemfile.lock"
        ) {
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
                    "npm" => Some(format!("npm install {package}")),
                    "pypi" => Some(format!("pip install {package}")),
                    "crates.io" => Some(format!("cargo add {package}")),
                    _ => None,
                };
                findings.push(Finding {
                    detector: "phantom-deps".to_string(),
                    severity: "blocking",
                    file: file.path.clone(),
                    line: import.line,
                    column: import.column,
                    message: format!(
                        "Package `{package}` is used but not declared in your dependency file."
                    ),
                    suggestion: Some(format!(
                        "Add `{package}` to your package manager's dependency file."
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
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    for file in files {
        let path = file.path.to_lowercase();

        if path.ends_with("package.json") {
            let pkgs = crate::dep_parser::extract_npm_deps(&file.raw_content);
            map.entry("npm".to_string()).or_default().extend(pkgs);
        } else if path.ends_with("requirements.txt")
            || path.ends_with("setup.py")
            || path.ends_with("setup.cfg")
        {
            let pkgs = crate::dep_parser::extract_requirements_deps(&file.raw_content);
            map.entry("pypi".to_string()).or_default().extend(pkgs);
        } else if path.ends_with("pyproject.toml") {
            let pkgs = crate::dep_parser::extract_pyproject_deps(&file.raw_content);
            map.entry("pypi".to_string()).or_default().extend(pkgs);
        } else if path.ends_with("cargo.toml") {
            let pkgs = crate::dep_parser::extract_cargo_deps(&file.raw_content);
            map.entry("crates.io".to_string()).or_default().extend(pkgs);
        } else if path.ends_with("go.mod") {
            let pkgs = crate::dep_parser::extract_go_mod_deps(&file.raw_content);
            map.entry("go".to_string()).or_default().extend(pkgs);
        }
    }

    map
}
