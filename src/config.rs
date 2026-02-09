use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bytesize::ByteSize;
use serde::Deserialize;

use crate::cli::Cli;

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Plain,
    Markdown,
    Xml,
    Json,
}

impl OutputFormat {
    pub fn from_str_loose(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "plain" | "txt" | "text" => Ok(Self::Plain),
            "markdown" | "md" => Ok(Self::Markdown),
            "xml" => Ok(Self::Xml),
            "json" => Ok(Self::Json),
            other => Err(anyhow!("Unknown output format: '{}'. Use: plain, markdown, xml, json", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Code block language strategy
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum CodeBlockLang {
    /// Use file extension as the language tag
    #[default]
    Auto,
    /// No language tag on code fences
    None,
    /// Force a specific language tag
    #[serde(untagged)]
    Custom(String),
}

// ---------------------------------------------------------------------------
// Partial config (layerable — all fields optional)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, Default, Clone)]
pub struct PartialConfig {
    /// Languages to load presets for
    #[serde(default)]
    pub language: Option<Vec<String>>,

    /// Named presets to extend from
    #[serde(default)]
    pub extends: Option<Vec<String>>,

    /// Include patterns (whitelist). If set, only matching files are considered.
    #[serde(default)]
    pub includes: Option<Vec<String>>,

    /// Ignore patterns (gitignore-style). Replaces previous layer's ignores.
    #[serde(default)]
    pub ignores: Option<Vec<String>>,

    /// Additional ignore patterns. Always appended, never replaces.
    #[serde(default)]
    pub extra_ignores: Option<Vec<String>>,

    #[serde(default)]
    pub recursive: Option<bool>,

    #[serde(default)]
    pub follow_symlinks: Option<bool>,

    #[serde(default)]
    pub skip_binary: Option<bool>,

    #[serde(default)]
    pub max_file_size: Option<String>,

    #[serde(default)]
    pub header_template: Option<String>,

    #[serde(default)]
    pub footer_template: Option<String>,

    #[serde(default)]
    pub output: Option<String>,

    #[serde(default)]
    pub format: Option<OutputFormat>,

    #[serde(default)]
    pub wrap_code_block: Option<bool>,

    #[serde(default)]
    pub code_block_lang: Option<CodeBlockLang>,

    #[serde(default)]
    pub toc: Option<bool>,
}

impl PartialConfig {
    /// Merge `other` on top of `self`. Fields in `other` take precedence.
    pub fn merge(&mut self, other: &PartialConfig) {
        macro_rules! merge_field {
            ($field:ident) => {
                if other.$field.is_some() {
                    self.$field = other.$field.clone();
                }
            };
        }

        merge_field!(language);
        merge_field!(extends);
        merge_field!(includes);
        merge_field!(recursive);
        merge_field!(follow_symlinks);
        merge_field!(skip_binary);
        merge_field!(max_file_size);
        merge_field!(header_template);
        merge_field!(footer_template);
        merge_field!(output);
        merge_field!(format);
        merge_field!(wrap_code_block);
        merge_field!(code_block_lang);
        merge_field!(toc);

        // `ignores` replaces entirely
        if other.ignores.is_some() {
            self.ignores = other.ignores.clone();
        }

        // `extra_ignores` always appends
        if let Some(ref extra) = other.extra_ignores {
            let mut current = self.extra_ignores.clone().unwrap_or_default();
            current.extend(extra.clone());
            self.extra_ignores = Some(current);
        }
    }
}

// ---------------------------------------------------------------------------
// Resolved config (all fields have concrete values)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Config {
    pub includes: Vec<String>,
    pub ignores: Vec<String>,
    pub recursive: bool,
    pub follow_symlinks: bool,
    pub skip_binary: bool,
    pub max_file_size: Option<ByteSize>,
    pub header_template: String,
    pub footer_template: String,
    pub output: String,
    pub format: OutputFormat,
    pub wrap_code_block: bool,
    pub code_block_lang: CodeBlockLang,
    pub toc: bool,
}

impl Config {
    /// Resolve a PartialConfig into a fully concrete Config, applying format-aware defaults.
    pub fn from_partial(partial: PartialConfig) -> Result<Self> {
        let format = partial.format.unwrap_or_default();

        // Format-aware defaults
        let (default_header, default_footer, default_wrap, default_output) = match format {
            OutputFormat::Plain => (
                "// File: {relative_path}\n".to_string(),
                "\n".to_string(),
                false,
                "unified.txt".to_string(),
            ),
            OutputFormat::Markdown => (
                "\n## {relative_path}\n\n".to_string(),
                "\n".to_string(),
                true,
                "unified.md".to_string(),
            ),
            OutputFormat::Xml => (
                "<file path=\"{relative_path}\" lines=\"{lines}\" size=\"{size}\">\n<![CDATA[\n".to_string(),
                "]]>\n</file>\n\n".to_string(),
                false,
                "unified.xml".to_string(),
            ),
            OutputFormat::Json => (
                String::new(), // JSON format doesn't use templates
                String::new(),
                false,
                "unified.json".to_string(),
            ),
        };

        // Combine ignores + extra_ignores
        let mut ignores = partial.ignores.unwrap_or_default();
        if let Some(extra) = partial.extra_ignores {
            ignores.extend(extra);
        }

        // Parse max_file_size
        let max_file_size = match partial.max_file_size {
            Some(ref s) => {
                let parsed: ByteSize = s.parse()
                    .map_err(|_| anyhow!("Invalid max_file_size: '{}'. Use e.g. '512KB', '2MB'", s))?;
                Some(parsed)
            }
            None => None,
        };

        Ok(Config {
            includes: partial.includes.unwrap_or_default(),
            ignores,
            recursive: partial.recursive.unwrap_or(true),
            follow_symlinks: partial.follow_symlinks.unwrap_or(false),
            skip_binary: partial.skip_binary.unwrap_or(true),
            max_file_size,
            header_template: partial.header_template.unwrap_or(default_header),
            footer_template: partial.footer_template.unwrap_or(default_footer),
            output: partial.output.unwrap_or(default_output),
            format,
            wrap_code_block: partial.wrap_code_block.unwrap_or(default_wrap),
            code_block_lang: partial.code_block_lang.unwrap_or_default(),
            toc: partial.toc.unwrap_or(false),
        })
    }
}

// ---------------------------------------------------------------------------
// Preset system
// ---------------------------------------------------------------------------

/// Canonical preset name → embedded TOML content
fn builtin_presets() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("rust", include_str!("../presets/rust.toml"));
    m.insert("python", include_str!("../presets/python.toml"));
    m.insert("javascript", include_str!("../presets/javascript.toml"));
    m.insert("go", include_str!("../presets/go.toml"));
    m.insert("java", include_str!("../presets/java.toml"));
    m.insert("c", include_str!("../presets/c.toml"));
    m.insert("csharp", include_str!("../presets/csharp.toml"));
    m.insert("php", include_str!("../presets/php.toml"));
    m.insert("ruby", include_str!("../presets/ruby.toml"));
    m.insert("swift", include_str!("../presets/swift.toml"));
    m.insert("elixir", include_str!("../presets/elixir.toml"));
    m
}

/// Alias → canonical preset name
fn preset_aliases() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("rs", "rust");
    m.insert("py", "python");
    m.insert("js", "javascript");
    m.insert("ts", "javascript");
    m.insert("typescript", "javascript");
    m.insert("node", "javascript");
    m.insert("golang", "go");
    m.insert("cpp", "c");
    m.insert("c++", "c");
    m.insert("cs", "csharp");
    m.insert("dotnet", "csharp");
    m.insert("rb", "ruby");
    m.insert("ex", "elixir");
    m
}

/// Resolve a language name to its canonical preset name
pub fn resolve_preset_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let aliases = preset_aliases();
    aliases
        .get(lower.as_str())
        .map(|s| s.to_string())
        .unwrap_or(lower)
}

/// Load a language preset: user override first, then built-in fallback
pub fn load_preset(name: &str) -> Result<PartialConfig> {
    let canonical = resolve_preset_name(name);

    // 1. Check user's custom preset
    if let Ok(config_dir) = global_config_dir() {
        let user_path = config_dir.join("languages").join(format!("{}.toml", canonical));
        if user_path.exists() {
            return load_partial_config(&user_path);
        }
    }

    // 2. Fall back to built-in
    let presets = builtin_presets();
    if let Some(content) = presets.get(canonical.as_str()) {
        let config: PartialConfig = toml::from_str(content)
            .with_context(|| format!("Bug: invalid built-in preset '{}'", canonical))?;
        return Ok(config);
    }

    // 3. Not found
    let available: Vec<&str> = {
        let mut names: Vec<&str> = presets.keys().copied().collect();
        names.sort();
        names
    };
    Err(anyhow!(
        "Unknown language '{}'. Available: {}",
        name,
        available.join(", ")
    ))
}

/// Get the raw TOML content for a built-in preset (for export)
pub fn builtin_preset_content(name: &str) -> Option<&'static str> {
    let canonical = resolve_preset_name(name);
    builtin_presets().get(canonical.as_str()).copied()
}

/// List all available preset names (built-in + user custom)
pub fn list_presets() -> Result<Vec<(String, bool)>> {
    let mut results: Vec<(String, bool)> = Vec::new();

    // Built-in presets
    let presets = builtin_presets();
    let mut names: Vec<&str> = presets.keys().copied().collect();
    names.sort();
    for name in names {
        results.push((name.to_string(), false));
    }

    // User custom presets
    if let Ok(config_dir) = global_config_dir() {
        let lang_dir = config_dir.join("languages");
        if lang_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&lang_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();
                        // Mark as custom override or purely custom
                        let is_override = presets.contains_key(name.as_str());
                        if !is_override {
                            results.push((name, true));
                        } else {
                            // Replace the built-in entry with "overridden" marker
                            if let Some(entry) = results.iter_mut().find(|(n, _)| n == &name) {
                                entry.1 = true;
                            }
                        }
                    }
                }
            }
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

// ---------------------------------------------------------------------------
// Auto-detection
// ---------------------------------------------------------------------------

/// Detect languages from project files in the working directory
pub fn detect_languages(working_dir: &Path) -> Vec<String> {
    let indicators: &[(&str, &str)] = &[
        ("Cargo.toml", "rust"),
        ("package.json", "javascript"),
        ("tsconfig.json", "javascript"),
        ("pyproject.toml", "python"),
        ("requirements.txt", "python"),
        ("setup.py", "python"),
        ("go.mod", "go"),
        ("pom.xml", "java"),
        ("build.gradle", "java"),
        ("build.gradle.kts", "java"),
        ("CMakeLists.txt", "c"),
        ("Makefile", "c"),
        ("composer.json", "php"),
        ("Gemfile", "ruby"),
        ("Package.swift", "swift"),
        ("mix.exs", "elixir"),
    ];

    let mut seen = std::collections::HashSet::new();
    let mut languages = Vec::new();

    for (file, lang) in indicators {
        if working_dir.join(file).exists() && seen.insert(*lang) {
            languages.push(lang.to_string());
        }
    }

    // Also check for *.csproj files
    if let Ok(entries) = fs::read_dir(working_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Some(ext) = entry.path().extension() {
                if ext == "csproj" || ext == "sln" {
                    if seen.insert("csharp") {
                        languages.push("csharp".to_string());
                    }
                    break;
                }
            }
        }
    }

    languages
}

// ---------------------------------------------------------------------------
// Config file loading
// ---------------------------------------------------------------------------

pub fn global_config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("unify"))
        .ok_or_else(|| anyhow!("Could not determine config directory"))
}

pub fn load_partial_config(path: &Path) -> Result<PartialConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse config: {}", path.display()))
}

/// Walk up from `start` looking for a config file
pub fn find_project_config(start: &Path, config_name: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(config_name);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// Full resolution pipeline
// ---------------------------------------------------------------------------

/// Build CLI overrides as a PartialConfig
fn cli_to_partial(cli: &Cli) -> Result<PartialConfig> {
    let mut p = PartialConfig::default();

    if let Some(ref langs) = cli.language {
        p.language = Some(langs.clone());
    }
    if let Some(ref size_str) = cli.max_size {
        p.max_file_size = Some(size_str.clone());
    }
    if cli.follow_symlinks {
        p.follow_symlinks = Some(true);
    }
    if let Some(ref output) = cli.output {
        p.output = Some(output.to_string_lossy().to_string());
    }
    if let Some(ref fmt) = cli.format {
        p.format = Some(OutputFormat::from_str_loose(fmt)?);
    }

    Ok(p)
}

/// Resolve the full config from all layers:
/// built-in defaults → global config → project config → language presets → CLI overrides
pub fn resolve_config(cli: &Cli, working_dir: &Path) -> Result<Config> {
    let mut merged = PartialConfig::default();

    // Layer 1: global config (~/.config/unify/config.toml)
    if let Ok(global_dir) = global_config_dir() {
        let global_path = global_dir.join("config.toml");
        if global_path.exists() {
            let global = load_partial_config(&global_path)?;
            merged.merge(&global);
        }
    }

    // Layer 2: project config (walk upward to find it)
    let default_config_name = PathBuf::from(".unify.toml");
    let explicit_config = cli.config != default_config_name;

    let project_config_path = if explicit_config {
        // Explicit --config: use it directly
        let path = working_dir.join(&cli.config);
        if path.exists() {
            Some(path)
        } else {
            return Err(anyhow!("Config file not found: {}", path.display()));
        }
    } else {
        // Default: search upward
        find_project_config(working_dir, &cli.config)
    };

    let has_project_config = project_config_path.is_some();
    if let Some(ref path) = project_config_path {
        let project = load_partial_config(path)?;
        merged.merge(&project);
    }

    // Layer 3: CLI overrides (applied before language resolution so --language works)
    let cli_overrides = cli_to_partial(cli)?;
    merged.merge(&cli_overrides);

    // Layer 4: resolve language presets
    // If no language specified anywhere, try auto-detection
    let languages = merged.language.clone().unwrap_or_default();
    let languages = if languages.is_empty() {
        let detected = detect_languages(working_dir);
        if !detected.is_empty() {
            eprintln!("Auto-detected languages: {}", detected.join(", "));
        }
        detected
    } else {
        languages
    };

    for lang in &languages {
        let preset = load_preset(lang)?;
        // Language presets append via extra_ignores so they don't replace user patterns
        let adapted = PartialConfig {
            extra_ignores: preset.ignores.clone(),
            ..Default::default()
        };
        merged.merge(&adapted);
    }

    // Layer 5: resolve `extends` (named presets from user config dir)
    if let Some(ref extends) = merged.extends.clone() {
        for name in extends {
            let preset = load_preset(name)?;
            merged.merge(&preset);
        }
    }

    // Helpful message if running without any config
    if !has_project_config && merged.ignores.is_none() && languages.is_empty() {
        eprintln!("Note: No .unify.toml found, using built-in defaults.");
        eprintln!("  Run `unify init` to create a config, or use --language to set patterns.");
    }

    // Also respect UNIFY_OUTPUT env var
    if merged.output.is_none() {
        if let Ok(env_output) = std::env::var("UNIFY_OUTPUT") {
            merged.output = Some(env_output);
        }
    }

    Config::from_partial(merged)
}

// ---------------------------------------------------------------------------
// Init command helpers
// ---------------------------------------------------------------------------

pub fn default_project_config_content(languages: &[String]) -> String {
    let mut lines = Vec::new();

    // Language field
    if !languages.is_empty() {
        let lang_list: Vec<String> = languages.iter().map(|l| format!("\"{}\"", l)).collect();
        lines.push(format!("language = [{}]", lang_list.join(", ")));
    }

    lines.push(String::new());

    // Common ignores
    lines.push("ignores = [".to_string());
    lines.push("    # VCS and IDE".to_string());
    lines.push("    \".git/\",".to_string());
    lines.push("    \".vscode/\",".to_string());
    lines.push("    \".idea/\",".to_string());
    lines.push(String::new());
    lines.push("    # Unify output".to_string());
    lines.push("    \"/.unify.toml\",".to_string());
    lines.push("    \"/README.md\",".to_string());
    lines.push("]".to_string());
    lines.push(String::new());

    // Other settings
    lines.push("# extra_ignores = [\"my-custom-dir/\"]".to_string());
    lines.push("# includes = [\"src/**\", \"Cargo.toml\"]".to_string());
    lines.push(String::new());
    lines.push("recursive = true".to_string());
    lines.push("follow_symlinks = false".to_string());
    lines.push("skip_binary = true".to_string());
    lines.push("# max_file_size = \"1MB\"".to_string());
    lines.push(String::new());
    lines.push("# format = \"plain\"  # plain, markdown, xml, json".to_string());
    lines.push("# output = \"unified.txt\"".to_string());
    lines.push("# wrap_code_block = false".to_string());
    lines.push("# code_block_lang = \"auto\"".to_string());
    lines.push("# toc = false".to_string());
    lines.push(String::new());
    lines.push("header_template = \"// File: {relative_path}\\n\"".to_string());
    lines.push("# footer_template = \"\\n\"".to_string());

    lines.join("\n") + "\n"
}

pub fn default_global_config_content() -> String {
    let mut lines = Vec::new();
    lines.push("# Unify global configuration".to_string());
    lines.push("# Applied to all projects unless overridden by a project .unify.toml".to_string());
    lines.push(String::new());
    lines.push("# language = [\"rust\"]".to_string());
    lines.push(String::new());
    lines.push("ignores = [".to_string());
    lines.push("    \".git/\",".to_string());
    lines.push("    \".vscode/\",".to_string());
    lines.push("    \".idea/\",".to_string());
    lines.push("    \"/.unify.toml\",".to_string());
    lines.push("]".to_string());
    lines.push(String::new());
    lines.push("recursive = true".to_string());
    lines.push("follow_symlinks = false".to_string());
    lines.push("skip_binary = true".to_string());
    lines.push("# max_file_size = \"1MB\"".to_string());
    lines.push(String::new());
    lines.push("# format = \"markdown\"".to_string());
    lines.push("# output = \"unified.md\"".to_string());
    lines.push("# wrap_code_block = true".to_string());
    lines.push("# toc = false".to_string());
    lines.push(String::new());
    lines.push("header_template = \"// File: {relative_path}\\n\"".to_string());

    lines.join("\n") + "\n"
}
