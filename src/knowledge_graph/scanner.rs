//! Code scanner — uses ast-grep (sg) CLI to extract symbols and relationships.
//! Users must install ast-grep separately: `cargo install ast-grep`
//!
//! For each supported language, runs `sg run --kind <kind> --json` for each
//! relevant AST node kind, parses the JSON output, and returns symbols/relations.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// A single symbol extracted from code.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
}

/// A relationship between two symbols.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Relation {
    pub source: String,
    pub target: String,
    pub relation: String,
}

/// Language config: (sg_language_name, [ast_kinds], import_kind)
const LANGUAGES: &[(&str, &[&str], &[&str])] = &[
    ("rust", &["struct_item", "function_item", "enum_item", "trait_item", "type_item", "impl_item", "const_item"], &["use_declaration"]),
    ("python", &["class_definition", "function_definition"], &["import_statement", "import_from_statement"]),
    ("typescript", &["class_declaration", "interface_declaration", "function_declaration", "enum_declaration", "type_alias_declaration"], &["import_statement"]),
    ("javascript", &["class_declaration", "function_declaration"], &["import_statement"]),
    ("java", &["class_declaration", "interface_declaration", "enum_declaration", "record_declaration", "method_declaration"], &["import_declaration"]),
    ("go", &["function_declaration", "method_declaration", "type_spec"], &["import_declaration"]),
    ("ruby", &["class", "module", "method"], &[]),
    ("php", &["class_declaration", "function_definition", "interface_declaration"], &[]),
    ("swift", &["class_declaration", "struct_declaration", "enum_declaration", "protocol_declaration", "function_declaration"], &[]),
    ("kotlin", &["class_declaration", "function_declaration", "interface_declaration"], &[]),
    ("c", &["function_definition", "struct_specifier"], &[]),
    ("cpp", &["function_definition", "class_specifier", "struct_specifier"], &[]),
    ("c-sharp", &["class_declaration", "interface_declaration", "method_declaration"], &[]),
    ("scala", &["class_definition", "object_definition", "trait_definition", "function_definition"], &[]),
    ("objc", &["interface_declaration", "implementation_declaration", "method_definition"], &[]),
    ("bash", &["function_definition"], &[]),
];

/// Scan specific files using ast-grep CLI. Only processes the given files.
pub fn scan_files(files: &[std::path::PathBuf]) -> Result<(Vec<Symbol>, Vec<Relation>)> {
    let root = files.first().map(|f| f.parent().unwrap_or(Path::new("."))).unwrap_or(Path::new("."));
    scan_directory_inner(root, Some(files))
}

/// Scan a directory using ast-grep CLI.
pub fn scan_directory(root: &Path) -> Result<(Vec<Symbol>, Vec<Relation>)> {
    scan_directory_inner(root, None)
}

fn scan_directory_inner(root: &Path, filtered_files: Option<&[std::path::PathBuf]>) -> Result<(Vec<Symbol>, Vec<Relation>)> {
    // Check sg is available
    let sg_check = Command::new("sg").arg("--version").output();
    if sg_check.is_err() {
        return Err(anyhow::anyhow!(
            "ast-grep (sg) not found. Install with: cargo install ast-grep"
        ));
    }

    let mut all_symbols = Vec::new();
    let mut all_relations = Vec::new();

    for (sg_lang, kinds, import_kinds) in LANGUAGES {
        // Extract symbols for each kind
        for kind in *kinds {
            let output = Command::new("sg")
                .args(["run", "--kind", kind, "--lang", sg_lang, "--json"])
                .arg(root.as_os_str())
                .output()
                .with_context(|| format!("sg run failed for lang={sg_lang} kind={kind}"))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("unknown language") || stderr.contains("language") {
                    log::warn!("sg: unknown language '{sg_lang}', skipping");
                } else if !stderr.contains("no") && !stderr.is_empty() {
                    log::warn!("sg: kind='{kind}' lang='{sg_lang}': {stderr}");
                }
                continue;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                continue;
            }

            let items: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
                Ok(v) => v,
                Err(e) => {
                    log::debug!("sg: JSON parse error for {sg_lang}/{kind}: {e}");
                    continue;
                }
            };

            for item in &items {
                let file = item["file"].as_str().unwrap_or("");
                let text = item["text"].as_str().unwrap_or("");
                let line = item["range"]["start"]["line"].as_u64().unwrap_or(0) as usize + 1;
                if file.is_empty() || text.is_empty() {
                    continue;
                }

                let name = extract_name(text);
                let file_stem = Path::new(file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                let qualified = if name == file_stem {
                    name.clone()
                } else {
                    format!("{}::{}", file_stem, name)
                };
                let node_kind = kind_to_symbol_kind(kind);

                let file_node = format!("file:{}", file_stem);
                all_symbols.push(Symbol {
                    name: qualified.clone(),
                    kind: node_kind,
                    file: file.to_string(),
                    line,
                });
                all_relations.push(Relation {
                    source: file_node,
                    target: qualified,
                    relation: "contains".into(),
                });
            }
        }

        // Extract import edges
        for import_kind in *import_kinds {
            let output = Command::new("sg")
                .args(["run", "--kind", import_kind, "--lang", sg_lang, "--json"])
                .arg(root.as_os_str())
                .output();

            if let Ok(output) = output {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.trim().is_empty() {
                        continue;
                    }
                    if let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                        for item in &items {
                            let file = item["file"].as_str().unwrap_or("");
                            let text = item["text"].as_str().unwrap_or("");
                            if file.is_empty() || text.is_empty() {
                                continue;
                            }
                            let file_stem = Path::new(file)
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let file_node = format!("file:{}", file_stem);
                            let import_name = extract_import_target(text, sg_lang);
                            if !import_name.is_empty() {
                                // Dedup imports from same file
                                let dup = all_relations.iter().any(|r| {
                                    r.source == file_node && r.target == import_name && r.relation == "imports"
                                });
                                if !dup {
                                    all_relations.push(Relation {
                                        source: file_node,
                                        target: import_name,
                                        relation: "imports".into(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Markdown: simple heading extraction (no ast-grep needed)
    let md_files = collect_md_files(root);
    for file_path in &md_files {
        let source = std::fs::read_to_string(file_path).unwrap_or_default();
        let file_stem = file_path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let file_node = format!("file:{}", file_stem);
        all_symbols.push(Symbol {
            name: file_node.clone(),
            kind: "file".into(),
            file: file_path.to_string_lossy().to_string(),
            line: 1,
        });

        for (line_no, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let level = trimmed.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&level) {
                let text = trimmed[level..].trim().to_string();
                if !text.is_empty() {
                    all_symbols.push(Symbol {
                        name: text.clone(),
                        kind: format!("heading.{}", level),
                        file: file_path.to_string_lossy().to_string(),
                        line: line_no + 1,
                    });
                    all_relations.push(Relation {
                        source: file_node.clone(),
                        target: text,
                        relation: "contains".into(),
                    });
                }
            }
        }
    }

    Ok((all_symbols, all_relations))
}

pub fn extract_name(text: &str) -> String {
    let text = text.trim();
    let keywords = [
        "pub", "async", "unsafe", "extern", "fn", "struct", "enum", "trait",
        "impl", "type", "const", "static", "module", "class", "def",
        "interface", "abstract", "sealed", "open", "data", "case",
        "object", "record", "protocol", "extension", "where", "for",
        "public", "private", "protected", "internal", "override",
        "virtual", "inline", "export", "declare", "mut", "ref", "let",
    ];
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for token in &tokens {
        // Extract alphanumeric + underscore prefix (drop generics, parens, etc.)
        let clean: String = token.trim().chars()
            .skip_while(|c| *c == '(' || *c == '<' || *c == '\'')
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if clean.is_empty() || keywords.contains(&clean.as_str()) || clean.starts_with("pub(") {
            continue;
        }
        return clean;
    }
    // Fallback: look for identifier after keyword patterns
    let triggers = ["fn ", "struct ", "enum ", "trait ", "class ", "def ", "interface ", "type "];
    for t in &triggers {
        if let Some(rest) = text.split_once(t).map(|(_, r)| r.trim()) {
            let name: String = rest.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '!' || *c == '?')
                .collect();
            if !name.is_empty() && !keywords.contains(&name.as_str()) {
                return name;
            }
        }
    }
    text.to_string()
}



pub fn extract_import_target(text: &str, lang: &str) -> String {
    match lang {
        "rust" => text.strip_prefix("use ")
            .and_then(|s| s.split("::").next())
            .unwrap_or("").to_string(),
        "python" => text.strip_prefix("import ")
            .or_else(|| text.strip_prefix("from "))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.split('.').next())
            .unwrap_or("").to_string(),
        "java" => text.strip_prefix("import ")
            .and_then(|s| s.split('.').next())
            .unwrap_or("").to_string(),
        "typescript" | "javascript" => text
            .split('\'').nth(1)
            .or_else(|| text.split('"').nth(1))
            .and_then(|s| s.split('/').next())
            .unwrap_or("").to_string(),
        "go" => text.split('"').nth(1).unwrap_or("").to_string(),
        _ => String::new(),
    }
}

pub fn kind_to_symbol_kind(kind: &str) -> String {
    match kind {
        "struct_item" | "struct_specifier" | "struct_declaration" => "struct".into(),
        "function_item" | "function_definition" | "function_declaration" | "method_declaration"
        | "method_definition" | "arrow_function" => "function".into(),
        "enum_item" | "enum_declaration" => "enum".into(),
        "trait_item" | "trait_definition" => "trait".into(),
        "impl_item" => "impl".into(),
        "class_declaration" | "class_definition" | "class_specifier" => "class".into(),
        "interface_declaration" | "protocol_declaration" => "interface".into(),
        "type_alias_declaration" | "type_item" | "type_spec" => "type_alias".into(),
        "const_item" => "constant".into(),
        "static_item" => "static".into(),
        "record_declaration" => "record".into(),
        "module" | "object_definition" => "module".into(),
        _ => kind.to_string(),
    }
}

fn collect_md_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if !root.is_dir() { return files; }
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if !name.starts_with('.') && name != "node_modules" && name != "target" {
                        dirs.push(path);
                    }
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext == "md" || ext == "mdx" {
                        files.push(path);
                    }
                }
            }
        }
    }
    files.sort();
    files
}
