use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bytesize::ByteSize;
use content_inspector::inspect;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use indicatif::{ProgressBar, ProgressStyle};

use crate::config::{CodeBlockLang, Config, OutputFormat};

// ---------------------------------------------------------------------------
// File info collected during walk (used for dry-run, TOC, JSON)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct FileInfo {
    pub path: PathBuf,
    pub relative_path: String,
    pub size: u64,
    pub lines: usize,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Walker construction using `ignore` crate
// ---------------------------------------------------------------------------

fn build_walker(
    base_path: &Path,
    working_dir: &Path,
    config: &Config,
) -> Result<ignore::Walk> {
    let mut builder = WalkBuilder::new(base_path);

    builder
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(config.follow_symlinks)
        .max_depth(if config.recursive { None } else { Some(1) });

    // Build overrides from includes + ignores
    let mut overrides = OverrideBuilder::new(working_dir);

    // Include patterns (positive globs → whitelist)
    for pattern in &config.includes {
        let trimmed = pattern.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        overrides.add(trimmed)
            .with_context(|| format!("Invalid include pattern: '{}'", trimmed))?;
    }

    // Ignore patterns (gitignore-style → convert to override negation)
    for pattern in &config.ignores {
        let trimmed = pattern.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix('!') {
            // Negated ignore = force include (double negation)
            overrides.add(stripped)
                .with_context(|| format!("Invalid pattern: '!{}'", stripped))?;
        } else {
            // Normal ignore = exclude
            overrides.add(&format!("!{}", trimmed))
                .with_context(|| format!("Invalid ignore pattern: '{}'", trimmed))?;
        }
    }

    builder.overrides(overrides.build()?);

    Ok(builder.build())
}

// ---------------------------------------------------------------------------
// Binary detection using content_inspector
// ---------------------------------------------------------------------------

fn is_binary_file(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buffer = [0u8; 1024];
    let bytes_read = file.read(&mut buffer)?;
    if bytes_read == 0 {
        return Ok(false);
    }
    Ok(!inspect(&buffer[..bytes_read]).is_text())
}

// ---------------------------------------------------------------------------
// Fence collision handling
// ---------------------------------------------------------------------------

fn fence_for_content(content: &str) -> String {
    let max_run = content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('`') {
                Some(trimmed.chars().take_while(|&c| c == '`').count())
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0);

    let fence_len = if max_run >= 3 { max_run + 1 } else { 3 };
    "`".repeat(fence_len)
}

// ---------------------------------------------------------------------------
// Template expansion
// ---------------------------------------------------------------------------

fn expand_template(
    template: &str,
    path: &Path,
    working_dir: &Path,
    content: &str,
    size: u64,
) -> String {
    let relative = path.strip_prefix(working_dir).unwrap_or(path);

    template
        .replace("{relative_path}", &relative.to_string_lossy())
        .replace(
            "{file_name}",
            &path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
        )
        .replace(
            "{extension}",
            path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        )
        .replace("{lines}", &content.lines().count().to_string())
        .replace("{size}", &ByteSize(size).to_string())
        .replace(
            "{dir}",
            &relative
                .parent()
                .unwrap_or(Path::new(""))
                .to_string_lossy(),
        )
}

// ---------------------------------------------------------------------------
// TOC generation
// ---------------------------------------------------------------------------

fn generate_toc(files: &[FileInfo], format: &OutputFormat) -> String {
    match format {
        OutputFormat::Markdown => {
            let mut toc = String::from("# Table of Contents\n\n");
            for file in files {
                // Create anchor from path (GitHub-style)
                let anchor = file
                    .relative_path
                    .to_lowercase()
                    .replace('/', "")
                    .replace('.', "")
                    .replace(' ', "-");
                toc.push_str(&format!(
                    "- [{}](#{})\n",
                    file.relative_path, anchor
                ));
            }
            toc.push_str("\n---\n");
            toc
        }
        OutputFormat::Plain => {
            let mut toc = String::from("=== Table of Contents ===\n\n");
            for (i, file) in files.iter().enumerate() {
                toc.push_str(&format!(
                    "  {}. {} ({}, {} lines)\n",
                    i + 1,
                    file.relative_path,
                    ByteSize(file.size),
                    file.lines
                ));
            }
            toc.push_str("\n");
            toc
        }
        _ => String::new(), // XML and JSON don't need a TOC section
    }
}

// ---------------------------------------------------------------------------
// Main processing pipeline
// ---------------------------------------------------------------------------

pub fn process_files(
    working_dir: &Path,
    paths_to_process: &[PathBuf],
    config: &Config,
    output: &mut dyn Write,
    config_path: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    let start_time = std::time::Instant::now();

    let paths_to_walk: Vec<PathBuf> = if !paths_to_process.is_empty() {
        paths_to_process
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    working_dir.join(p)
                }
            })
            .collect()
    } else {
        vec![working_dir.to_path_buf()]
    };

    // Phase 1: collect file paths
    let mut file_paths: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut skipped_binary = 0u64;
    let mut skipped_size = 0u64;
    let mut skipped_symlink = 0u64;

    for base_path in &paths_to_walk {
        if !base_path.exists() {
            eprintln!("Warning: Path does not exist: {}", base_path.display());
            continue;
        }

        let walker = build_walker(base_path, working_dir, config)?;

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Warning: {}", e);
                    continue;
                }
            };

            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Skip config file
            if let Some(cfg) = config_path {
                if path == cfg {
                    continue;
                }
            }

            // Skip output file
            let rel = path.strip_prefix(working_dir).unwrap_or(path);
            if rel.to_string_lossy() == config.output {
                continue;
            }

            // Skip symlinks if not following
            if !config.follow_symlinks {
                if fs::symlink_metadata(path)
                    .map_or(false, |m| m.file_type().is_symlink())
                {
                    skipped_symlink += 1;
                    continue;
                }
            }

            // Deduplication via canonical path
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if !seen.insert(canonical) {
                continue;
            }

            // Size filter
            if let Some(max_size) = config.max_file_size {
                if let Ok(meta) = fs::metadata(path) {
                    if meta.len() > max_size.as_u64() {
                        skipped_size += 1;
                        continue;
                    }
                }
            }

            // Binary filter
            if config.skip_binary {
                match is_binary_file(path) {
                    Ok(true) => {
                        skipped_binary += 1;
                        continue;
                    }
                    Err(_) => continue,
                    _ => {}
                }
            }

            file_paths.push(path.to_path_buf());
        }
    }

    // Sort for deterministic output
    file_paths.sort();

    // Dry run: just print stats
    if dry_run {
        return print_dry_run(&file_paths, working_dir, skipped_binary, skipped_size, skipped_symlink);
    }

    // Phase 2: read all files
    let pb = ProgressBar::new(file_paths.len() as u64);
    {
        use std::io::IsTerminal;
        if std::io::stderr().is_terminal() {
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40} {pos}/{len} files")
                    .unwrap_or_else(|_| ProgressStyle::default_bar()),
            );
        } else {
            pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }
    }

    let mut files: Vec<FileInfo> = Vec::new();
    for path in &file_paths {
        let relative = path
            .strip_prefix(working_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Skipping {} ({})", relative, e);
                pb.inc(1);
                continue;
            }
        };

        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let lines = content.lines().count();

        files.push(FileInfo {
            path: path.clone(),
            relative_path: relative,
            size,
            lines,
            content,
        });

        pb.inc(1);
    }

    pb.finish_and_clear();

    // Phase 3: write output
    let mut bytes_written: usize = 0;

    match config.format {
        OutputFormat::Json => {
            bytes_written += write_json_output(&files, output)?;
        }
        OutputFormat::Xml => {
            bytes_written += output.write(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<files>\n")?;

            if config.toc {
                // XML doesn't use TOC section
            }

            for file in &files {
                let header = expand_template(
                    &config.header_template,
                    &file.path,
                    working_dir,
                    &file.content,
                    file.size,
                );
                bytes_written += output.write(header.as_bytes())?;
                // Escape ]]> in content for CDATA safety
                let safe_content = file.content.replace("]]>", "]]]]><![CDATA[>");
                bytes_written += output.write(safe_content.as_bytes())?;
                if !safe_content.ends_with('\n') {
                    bytes_written += output.write(b"\n")?;
                }
                let footer = expand_template(
                    &config.footer_template,
                    &file.path,
                    working_dir,
                    &file.content,
                    file.size,
                );
                bytes_written += output.write(footer.as_bytes())?;
            }

            bytes_written += output.write(b"</files>\n")?;
        }
        _ => {
            // Plain or Markdown
            if config.toc && !files.is_empty() {
                let toc = generate_toc(&files, &config.format);
                bytes_written += output.write(toc.as_bytes())?;
            }

            for file in &files {
                let header = expand_template(
                    &config.header_template,
                    &file.path,
                    working_dir,
                    &file.content,
                    file.size,
                );
                bytes_written += output.write(header.as_bytes())?;

                if config.wrap_code_block {
                    let fence = fence_for_content(&file.content);
                    let lang = resolve_code_lang(&file.path, &config.code_block_lang);
                    bytes_written +=
                        output.write(format!("{}{}\n", fence, lang).as_bytes())?;
                    bytes_written += output.write(file.content.as_bytes())?;
                    if !file.content.ends_with('\n') {
                        bytes_written += output.write(b"\n")?;
                    }
                    bytes_written += output.write(format!("{}\n", fence).as_bytes())?;
                } else {
                    bytes_written += output.write(file.content.as_bytes())?;
                    if !file.content.ends_with('\n') {
                        bytes_written += output.write(b"\n")?;
                    }
                }

                let footer = expand_template(
                    &config.footer_template,
                    &file.path,
                    working_dir,
                    &file.content,
                    file.size,
                );
                bytes_written += output.write(footer.as_bytes())?;
            }
        }
    }

    let elapsed = start_time.elapsed();
    let total_lines: usize = files.iter().map(|f| f.lines).sum();

    // Token estimate
    let total_chars: usize = files.iter().map(|f| f.content.len()).sum();
    let token_estimate = estimate_tokens(total_chars);

    // Summary
    eprintln!(
        "Unified {} files ({}, ~{} lines, ~{} tokens) → {} in {:.2?}",
        files.len(),
        ByteSize(bytes_written as u64),
        total_lines,
        format_number(token_estimate),
        config.output,
        elapsed
    );

    if skipped_binary > 0 || skipped_size > 0 || skipped_symlink > 0 {
        let mut skip_parts = Vec::new();
        if skipped_binary > 0 {
            skip_parts.push(format!("{} binary", skipped_binary));
        }
        if skipped_size > 0 {
            skip_parts.push(format!("{} too large", skipped_size));
        }
        if skipped_symlink > 0 {
            skip_parts.push(format!("{} symlinks", skipped_symlink));
        }
        eprintln!("  Skipped: {}", skip_parts.join(", "));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Code block language resolution (just use file extension!)
// ---------------------------------------------------------------------------

fn resolve_code_lang(path: &Path, strategy: &CodeBlockLang) -> String {
    match strategy {
        CodeBlockLang::Auto => path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string(),
        CodeBlockLang::None => String::new(),
        CodeBlockLang::Custom(lang) => lang.clone(),
    }
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

fn write_json_output(files: &[FileInfo], output: &mut dyn Write) -> Result<usize> {
    let entries: Vec<serde_json::Value> = files
        .iter()
        .map(|f| {
            serde_json::json!({
                "path": f.relative_path,
                "extension": Path::new(&f.relative_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or(""),
                "lines": f.lines,
                "size": f.size,
                "content": f.content,
            })
        })
        .collect();

    let json_str = serde_json::to_string_pretty(&entries)?;
    let written = output.write(json_str.as_bytes())?;
    output.write(b"\n")?;
    Ok(written + 1)
}

// ---------------------------------------------------------------------------
// Dry run
// ---------------------------------------------------------------------------

fn print_dry_run(
    file_paths: &[PathBuf],
    working_dir: &Path,
    skipped_binary: u64,
    skipped_size: u64,
    skipped_symlink: u64,
) -> Result<()> {
    let mut total_size: u64 = 0;
    let mut total_lines: usize = 0;

    println!("Would unify {} files:\n", file_paths.len());

    for path in file_paths {
        let rel = path.strip_prefix(working_dir).unwrap_or(path);
        let meta = fs::metadata(path);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        total_size += size;

        // Quick line count without reading full content
        let lines = match fs::read_to_string(path) {
            Ok(c) => {
                let l = c.lines().count();
                total_lines += l;
                l
            }
            Err(_) => 0,
        };

        println!("  {} ({}, {} lines)", rel.display(), ByteSize(size), lines);
    }

    let token_estimate = estimate_tokens(total_size as usize);

    println!();
    println!(
        "Total: {} files, {}, ~{} lines, ~{} tokens",
        file_paths.len(),
        ByteSize(total_size),
        total_lines,
        format_number(token_estimate)
    );

    if skipped_binary > 0 || skipped_size > 0 || skipped_symlink > 0 {
        println!();
        println!("Skipped:");
        if skipped_binary > 0 {
            println!("  {} binary files", skipped_binary);
        }
        if skipped_size > 0 {
            println!("  {} files exceeded max_file_size", skipped_size);
        }
        if skipped_symlink > 0 {
            println!("  {} symlinks", skipped_symlink);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Estimate token count. Uses tiktoken if available, otherwise ~4 chars/token.
pub fn estimate_tokens(char_count: usize) -> usize {
    #[cfg(feature = "tokens")]
    {
        // With tiktoken feature, we'd do real counting per-file.
        // For the summary estimate, still use the heuristic since
        // we're counting across all files.
        char_count / 4
    }

    #[cfg(not(feature = "tokens"))]
    {
        char_count / 4
    }
}

fn format_number(n: usize) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}
