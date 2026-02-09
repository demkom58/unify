mod cli;
mod config;
mod process;

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::Parser;

use cli::{Cli, Commands, PresetAction};
use config::{
    builtin_preset_content, default_global_config_content, default_project_config_content,
    global_config_dir, list_presets, load_preset, resolve_config, resolve_preset_name,
};

fn main() -> Result<()> {
    let cli = Cli::parse();

    let working_dir = cli
        .dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("Cannot determine current directory"));

    match &cli.command {
        Some(Commands::Init {
            language,
            force,
            global,
        }) => {
            return cmd_init(&working_dir, &cli, language, *force, *global);
        }
        Some(Commands::Preset { action }) => {
            return cmd_preset(action);
        }
        Some(Commands::Help) => {
            print_pattern_help();
            return Ok(());
        }
        None => {}
    }

    // Default: unify files
    let config = resolve_config(&cli, &working_dir)?;

    // Determine output writer
    let config_path = working_dir.join(&cli.config);
    let config_path_opt = if config_path.exists() {
        Some(config_path.as_path())
    } else {
        None
    };

    let mut output: Box<dyn io::Write> = if cli.dry_run {
        // Dry run writes nothing to file; process_files handles printing
        Box::new(io::sink())
    } else if let Some(ref output_path) = cli.output {
        Box::new(
            fs::File::create(output_path)
                .with_context(|| format!("Cannot create output file: {}", output_path.display()))?,
        )
    } else if config.output == "-" {
        Box::new(io::stdout())
    } else {
        let output_path = working_dir.join(&config.output);
        // Create parent directories if needed
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        Box::new(
            fs::File::create(&output_path)
                .with_context(|| format!("Cannot create output file: {}", output_path.display()))?,
        )
    };

    process::process_files(
        &working_dir,
        &cli.paths,
        &config,
        &mut output,
        config_path_opt,
        cli.dry_run,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Init command
// ---------------------------------------------------------------------------

fn cmd_init(
    working_dir: &Path,
    cli: &Cli,
    language: &Option<Vec<String>>,
    force: bool,
    global: bool,
) -> Result<()> {
    if global {
        let dir = global_config_dir()?;
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");

        if path.exists() && !force {
            return Err(anyhow!(
                "{} already exists. Use --force to overwrite.",
                path.display()
            ));
        }

        let content = default_global_config_content();
        fs::write(&path, content)?;
        println!("Created global config: {}", path.display());
        return Ok(());
    }

    let config_path = working_dir.join(&cli.config);

    if config_path.exists() && !force {
        return Err(anyhow!(
            "{} already exists. Use --force to overwrite.",
            config_path.display()
        ));
    }

    let languages = language.clone().unwrap_or_default();

    // Validate languages
    for lang in &languages {
        let canonical = resolve_preset_name(lang);
        // Try loading to check it exists
        load_preset(&canonical)?;
    }

    let content = default_project_config_content(&languages);
    fs::write(&config_path, content)?;

    println!("Created config: {}", config_path.display());
    if !languages.is_empty() {
        let canonical: Vec<String> = languages.iter().map(|l| resolve_preset_name(l)).collect();
        println!("Languages: {}", canonical.join(", "));
    }
    println!("\nRun `unify` to unify your project files.");
    println!("Run `unify --dry-run` to preview what would be included.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Preset command
// ---------------------------------------------------------------------------

fn cmd_preset(action: &PresetAction) -> Result<()> {
    match action {
        PresetAction::List => {
            let presets = list_presets()?;
            println!("Available presets:\n");
            for (name, is_custom) in &presets {
                let marker = if *is_custom { " (customized)" } else { "" };
                println!("  {}{}", name, marker);
            }
            println!("\nAliases: rs→rust, py→python, js/ts→javascript, cpp→c, cs→csharp, rb→ruby, ex→elixir");
            Ok(())
        }

        PresetAction::Show { name } => {
            let canonical = resolve_preset_name(name);
            let preset = load_preset(&canonical)?;

            println!("# Preset: {}\n", canonical);
            if let Some(ref ignores) = preset.ignores {
                println!("ignores = [");
                for pattern in ignores {
                    println!("    \"{}\",", pattern);
                }
                println!("]");
            }
            Ok(())
        }

        PresetAction::Export { name, force } => {
            let canonical = resolve_preset_name(name);

            // Get built-in content
            let content = builtin_preset_content(&canonical)
                .ok_or_else(|| anyhow!("No built-in preset for '{}'", canonical))?;

            let dir = global_config_dir()?.join("languages");
            fs::create_dir_all(&dir)?;

            let dest = dir.join(format!("{}.toml", canonical));
            if dest.exists() && !*force {
                return Err(anyhow!(
                    "{} already exists. Use --force to overwrite.",
                    dest.display()
                ));
            }

            fs::write(&dest, content)?;
            println!("Exported '{}' preset to: {}", canonical, dest.display());
            println!("Edit this file to customize the preset.");
            Ok(())
        }

        PresetAction::Reset { name } => {
            let canonical = resolve_preset_name(name);
            let dir = global_config_dir()?.join("languages");
            let path = dir.join(format!("{}.toml", canonical));

            if !path.exists() {
                println!("No custom preset for '{}'. Already using built-in.", canonical);
                return Ok(());
            }

            fs::remove_file(&path)?;
            println!("Reset '{}' to built-in preset.", canonical);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern help
// ---------------------------------------------------------------------------

fn print_pattern_help() {
    println!(
        r#"
Unify — Ignore Pattern Syntax Help

Patterns in .unify.toml follow gitignore-style rules:

BASIC RULES
  - Lines starting with # are comments
  - Blank lines are ignored
  - Patterns ending with / match directories only
  - Patterns starting with / are anchored to the project root
  - Patterns starting with ! negate a previous pattern (re-include)

WILDCARDS
  *   Matches anything except /
  ?   Matches any single character except /
  **  Matches any number of directories (crosses / boundaries)
  []  Character class (e.g. *.[oa] matches .o and .a files)

EXAMPLES
  *.log           Ignore all .log files anywhere
  /node_modules/  Ignore node_modules at the project root only
  build/          Ignore all directories named 'build'
  /dist/**        Ignore everything inside root 'dist' directory
  !important.log  Re-include important.log even if other patterns exclude it

INCLUDES
  The `includes` field acts as a whitelist. When set, ONLY files matching
  at least one include pattern are considered:

  includes = ["src/**", "Cargo.toml"]
  ignores = ["src/generated/**"]

CONFIG EXAMPLE
  ignores = [
      ".git/",
      "*.log",
      "/target/",
  ]
  recursive = true
  format = "markdown"
  output = "unified.md"
  wrap_code_block = true
  toc = true

TEMPLATE VARIABLES
  {{relative_path}}  Full path relative to project root  (src/main.rs)
  {{file_name}}      File name only                      (main.rs)
  {{extension}}      File extension                      (rs)
  {{lines}}          Line count                          (142)
  {{size}}           Human-readable size                 (4.2 KB)
  {{dir}}            Directory portion                   (src)
"#
    );
}

