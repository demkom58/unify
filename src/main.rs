use std::path::{Path, PathBuf};
use std::fs;
use std::io::{self, Read, Write};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    author = "Unify Tool Author",
    version,
    about = "A file unification tool that joins text files according to gitignore-style patterns",
    long_about = "Unify reads text files from your project directory and combines them into a single output file. \
                 It uses gitignore-style patterns to determine which files to include or exclude. \
                 This tool is useful for code reviews, documentation, and sharing code snippets.",
    after_help = "EXAMPLES:
    # Create a configuration file
    unify init --language rust

    # Unify files using default config
    unify

    # Unify only specific directories
    unify ./src ./lib

    # Specify a different config file and output
    unify -c custom-config.toml -o combined.txt

    # Process files in another directory
    unify -d /path/to/project"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output file path (default: use config output setting)
    #[arg(short, long, help = "Path where the unified output will be written")]
    output: Option<PathBuf>,

    /// Path to .unify.toml config file
    #[arg(short, long, default_value = ".unify.toml", help = "Path to the configuration file")]
    config: PathBuf,

    /// Working directory (default: current directory)
    #[arg(short, long, help = "Directory to process (defaults to current directory)")]
    dir: Option<PathBuf>,
    
    /// Specific directories to process (if not specified, process the whole working directory)
    #[arg(help = "Specific directories to process (if not specified, process the whole working directory)")]
    paths: Vec<PathBuf>,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Create a new .unify.toml configuration file
    #[command(
        about = "Initialize a new .unify.toml config file",
        long_about = "Creates a new configuration file with sensible defaults for your project. \
                     You can specify one or more programming languages to get language-specific ignore patterns."
    )]
    Init {
        /// Programming languages to configure for (comma-separated)
        #[arg(
            short, 
            long, 
            help = "Programming languages to configure for (comma-separated)",
            long_help = "Specifies the programming languages to generate appropriate ignore patterns for. \
                        Supported languages include: js/javascript/typescript, python, rust, go, java, \
                        c/c++, csharp/dotnet, php, ruby, swift, and elixir. \
                        Multiple languages can be specified with comma separation, e.g., 'rust,js,python'."
        )]
        language: Option<String>,
        
        /// Force overwrite of existing config file
        #[arg(
            short, 
            long, 
            help = "Overwrite existing config file if it exists"
        )]
        force: bool,
    },

    /// Show help about pattern syntax
    #[command(
        about = "Show help about pattern syntax",
        long_about = "Displays detailed help about the gitignore-style pattern syntax used in .unify.toml."
    )]
    Help,
}

#[derive(Deserialize, Debug)]
struct Config {
    #[serde(default)]
    ignores: Vec<String>,
    
    #[serde(default = "default_recursive")]
    recursive: bool,
    
    #[serde(default = "default_header_template")]
    header_template: String,
    
    #[serde(default = "default_output")]
    output: String,
}

fn default_recursive() -> bool {
    true
}

fn default_header_template() -> String {
    "// File: {relative_path}\n".to_string()
}

fn default_output() -> String {
    "unified.txt".to_string()
}

struct GitignorePattern {
    pattern: glob::Pattern,
    is_negated: bool,
    is_dir_only: bool,
}

impl GitignorePattern {
    fn new(pattern_str: &str) -> Result<Self, glob::PatternError> {
        let trimmed = pattern_str.trim();
        
        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            // Return a pattern that won't match anything
            return Ok(GitignorePattern {
                pattern: glob::Pattern::new("this-will-never-match-anything")?,
                is_negated: false,
                is_dir_only: false,
            });
        }
        
        // Check if pattern is negated (unignore)
        let is_negated = trimmed.starts_with('!');
        let trimmed = if is_negated { &trimmed[1..] } else { trimmed };
        
        // Check if pattern is anchored to root
        let is_root_only = trimmed.starts_with('/');
        let trimmed = if is_root_only { &trimmed[1..] } else { trimmed };
        
        // Check if pattern is directory-only
        let is_dir_only = trimmed.ends_with('/');
        let trimmed = if is_dir_only { &trimmed[..trimmed.len()-1] } else { trimmed };
        
        // Replace backslashes with forward slashes for cross-platform consistency
        let normalized = trimmed.replace('\\', "/");
        
        // Convert .gitignore pattern to glob pattern
        let glob_pattern = if is_root_only {
            normalized
        } else {
            // Non-root patterns can match anywhere in the path
            format!("**/{}", normalized)
        };
        
        Ok(GitignorePattern {
            pattern: glob::Pattern::new(&glob_pattern)?,
            is_negated,
            is_dir_only,
        })
    }
    
    fn matches(&self, path: &str, is_dir: bool) -> bool {
        // Directory-only patterns only match directories
        if self.is_dir_only && !is_dir {
            return false;
        }
        
        self.pattern.matches(path)
    }
}

struct GitignoreMatcher {
    patterns: Vec<GitignorePattern>,
}

impl GitignoreMatcher {
    fn is_match(&self, path: &Path, is_dir: bool) -> bool {
        let path_str = path.to_string_lossy().replace('\\', "/");
        
        // Default to including the file (not ignored)
        let mut result = true;
        
        // Process patterns in order
        for pattern in &self.patterns {
            if pattern.matches(&path_str, is_dir) {
                // If pattern matches, set result based on whether it's negated
                result = pattern.is_negated;
            }
        }
        
        result
    }
    
    fn should_skip_dir(&self, path: &Path) -> bool {
        // If the directory itself is excluded, we can skip it
        if !self.is_match(path, true) {
            // But first check if any negated patterns might apply to contents
            let path_str = path.to_string_lossy().replace('\\', "/");
            let path_with_trailing_slash = if path_str.ends_with('/') {
                path_str.to_string()
            } else {
                format!("{}/", path_str)
            };
            
            // Look for negated patterns that could match inside this directory
            for pattern in &self.patterns {
                if pattern.is_negated {
                    let pattern_str = pattern.pattern.as_str();
                    
                    // Check if this negated pattern could match something inside
                    if pattern_str.starts_with(&path_with_trailing_slash) || 
                       pattern_str.contains(&format!("/{}/", path_with_trailing_slash)) {
                        // Found a negated pattern that could match inside, so don't skip
                        return false;
                    }
                }
            }
            
            // No negated patterns would match inside, safe to skip
            return true;
        }
        
        false
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Set working directory
    let working_dir = cli.dir.unwrap_or_else(|| std::env::current_dir().unwrap());
    
    // Handle subcommands
    match &cli.command {
        Some(Commands::Init { language, force }) => {
            return init_config(&working_dir.join(&cli.config), language, *force);
        },
        Some(Commands::Help) => {
            print_pattern_help();
            return Ok(());
        },
        None => {
            // Default behavior - process files
        }
    }
    
    // Read config file
    let config_path = working_dir.join(&cli.config);
    let config = read_config(&config_path)?;
    
    // Determine output destination
    let mut output: Box<dyn Write> = if let Some(output_path) = cli.output.clone() {
        Box::new(fs::File::create(output_path)?)
    } else if config.output == "-" {
        Box::new(io::stdout())
    } else {
        Box::new(fs::File::create(working_dir.join(&config.output))?)
    };
    
    // Process files, either from specific paths or the whole working directory
    process_files(&working_dir, &cli.paths, &config, &mut output, &cli.config)?;
    
    Ok(())
}

fn read_config(path: &Path) -> Result<Config> {
    let config_str = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    
    let config: Config = toml::from_str(&config_str)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    
    Ok(config)
}

fn build_gitignore_matcher(config: &Config) -> Result<GitignoreMatcher> {
    let mut patterns = Vec::new();
    
    // Add standard patterns that should always be there
    let standard_patterns = vec![
        // Don't process binary files by default
        "*.exe", "*.dll", "*.so", "*.dylib", "*.o", "*.obj", 
        "*.bin", "*.dat", "*.zip", "*.tar", "*.gz", "*.rar",
        "*.jpg", "*.jpeg", "*.png", "*.gif", "*.bmp", "*.ico",
        "*.mp3", "*.mp4", "*.avi", "*.mov", "*.pdf"
    ];
    
    // Add standard patterns first
    for pattern_str in standard_patterns {
        match GitignorePattern::new(pattern_str) {
            Ok(pattern) => patterns.push(pattern),
            Err(e) => eprintln!("Warning: Invalid standard pattern '{}': {}", pattern_str, e),
        }
    }
    
    // Then add user-defined patterns
    for pattern_str in &config.ignores {
        match GitignorePattern::new(pattern_str) {
            Ok(pattern) => patterns.push(pattern),
            Err(e) => return Err(anyhow!("Invalid pattern '{}': {}", pattern_str, e)),
        }
    }
    
    Ok(GitignoreMatcher { patterns })
}

fn process_files(working_dir: &Path, paths_to_process: &[PathBuf], config: &Config, output: &mut dyn Write, config_filename: &Path) -> Result<()> {
    let start_time = std::time::Instant::now();
    let matcher = build_gitignore_matcher(config)?;
    
    // If specific paths are provided, process only those paths
    // Otherwise, process the entire working directory
    let paths_to_walk = if !paths_to_process.is_empty() {
        paths_to_process.to_vec()
    } else {
        vec![working_dir.to_path_buf()]
    };
    
    let mut total_files_processed = 0;
    let mut bytes_written = 0;
    
    for base_path in paths_to_walk {
        let full_path = if base_path.is_absolute() {
            base_path.clone()
        } else {
            working_dir.join(&base_path)
        };
        
        if !full_path.exists() {
            eprintln!("Warning: Path does not exist: {}", full_path.display());
            continue;
        }
        
        let walker = walkdir::WalkDir::new(&full_path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| {
                if !config.recursive && entry.depth() > 1 {
                    return false;
                }
                
                let path = entry.path();
                let relative_path = path.strip_prefix(working_dir).unwrap_or(path);
                
                // Skip the config file itself
                if path == working_dir.join(config_filename) {
                    return false;
                }
                
                // Skip the output file to avoid recursion
                if relative_path.to_string_lossy() == config.output {
                    return false;
                }
                
                if path.is_dir() {
                    // For directories, check if we can skip the entire directory
                    return !matcher.should_skip_dir(relative_path);
                }
                
                // For files, check if the path matches our patterns
                matcher.is_match(relative_path, false)
            });
        
        for entry in walker {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                continue;
            }
            
            let file_bytes = process_file(working_dir, path, config, output)?;
            bytes_written += file_bytes;
            total_files_processed += 1;
        }
    }
    
    let elapsed_time = start_time.elapsed();
    
    // Convert to appropriate unit for file size
    let size_display = if bytes_written < 1024 {
        format!("{} bytes", bytes_written)
    } else if bytes_written < 1024 * 1024 {
        format!("{:.2} KB", bytes_written as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes_written as f64 / (1024.0 * 1024.0))
    };
    
    // Print one-line summary
    println!("Unification complete: {} files processed, {} output size, {:.2?} time taken", 
             total_files_processed, size_display, elapsed_time);
    
    Ok(())
}

fn process_file(working_dir: &Path, path: &Path, config: &Config, output: &mut dyn Write) -> Result<usize> {
    // Skip binary files
    if is_binary_file(path)? {
        return Ok(0); // Return 0 bytes processed
    }
    
    let relative_path = path.strip_prefix(working_dir)
        .unwrap_or(path)
        .to_string_lossy();
    
    let file_name = path.file_name()
        .unwrap_or_default()
        .to_string_lossy();
    
    // Create header with placeholders replaced
    let header = config.header_template
        .replace("{relative_path}", &relative_path)
        .replace("{file_name}", &file_name);
    
    // Track bytes written
    let mut bytes_written = 0;
    
    // Write header
    bytes_written += output.write(header.as_bytes())?;
    
    // Try to read file content as UTF-8, handling errors gracefully
    match fs::read_to_string(path) {
        Ok(content) => {
            bytes_written += output.write(content.as_bytes())?;
            bytes_written += output.write(b"\n")?;
        },
        Err(e) => {
            // If it fails, add a comment noting the error
            let error_msg = format!("// ERROR: Could not read file as text: {}\n", e);
            bytes_written += output.write(error_msg.as_bytes())?;
            eprintln!("Skipping file with invalid UTF-8: {}", path.display());
        }
    }
    
    // Add a newline between files
    bytes_written += output.write(b"\n")?;
    
    Ok(bytes_written)
}

// A simple heuristic to detect binary files
fn is_binary_file(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 8192]; // Read 8KB to check
    
    let bytes_read = file.read(&mut buffer)?;
    if bytes_read == 0 {
        return Ok(false); // Empty file is not binary
    }
    
    // Check for null bytes or other binary indicators
    for &byte in &buffer[..bytes_read] {
        if byte == 0 {
            return Ok(true);
        }
    }
    
    // Count control characters (non-printable ASCII) except common whitespace
    let control_chars = buffer[..bytes_read].iter()
        .filter(|&&b| (b < 32 && ![9, 10, 13].contains(&b)) || b == 127)
        .count();
    
    // If more than 10% are control characters, consider it binary
    Ok((control_chars as f64 / bytes_read as f64) > 0.1)
}

fn init_config(config_path: &Path, language_option: &Option<String>, force: bool) -> Result<()> {
    // Check if the config file already exists
    if config_path.exists() && !force {
        return Err(anyhow!("Config file {} already exists. Use --force to overwrite.", config_path.display()));
    }
    
    // Common patterns for all languages
    let mut patterns = vec![
        "# Root-level only patterns",
        "/node_modules/",
        "/build/",
        "/dist/",
        "/.unify.toml",
        "/README.md",
        "",
        "# Patterns that apply everywhere",
        ".git/",
        ".vscode/",
        ".idea/",
        "*.exe",
        "*.dll",
        "*.so",
        "*.dylib",
        "*.o",
        "*.obj",
        "*.a",
        "*.lib",
        "*.out",
        "*.log",
        "*.svg",
        "*.png",
        "*.jpg",
        "*.jpeg",
        "*.gif",
        "*.ico",
        "*.woff",
        "*.woff2",
        "*.ttf",
        "*.eot",
        "*.zip",
        "*.tar.gz",
        "*.rar",
        "*.tgz",
    ];
    
    // Process languages
    let mut languages_added = Vec::new();
    if let Some(lang_str) = language_option {
        // Split by comma for multiple languages
        let languages: Vec<&str> = lang_str.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
            
        for lang in languages {
            let added = add_language_patterns(&mut patterns, lang);
            if added {
                languages_added.push(lang.to_string());
            }
        }
    }
    
    // Build ignores options
    let mut all_patterns = Vec::new();
    for pattern in patterns {
        if pattern.starts_with('#') || pattern.is_empty() {
            all_patterns.push(pattern.to_string());
        } else {
            all_patterns.push(format!("    \"{}\"", pattern));
        }
    }
    
    let config_content = format!(
        "ignores = [\n{}\n]\nrecursive = true\nheader_template = \"\\n---\\nFile: {{relative_path}}\\n---\\n\"\noutput = \"project.joined.txt\"\n",
        all_patterns.join(",\n")
    );
    
    // Write the config file
    fs::write(config_path, config_content)?;
    
    println!("Created config file: {}", config_path.display());
    
    if !languages_added.is_empty() {
        println!("Configured for languages: {}", languages_added.join(", "));
    }
    
    println!("\nTo unify your project files, run:\n  unify");
    println!("\nTo learn more about ignore patterns, run:\n  unify help");
    
    Ok(())
}

fn add_language_patterns(patterns: &mut Vec<&str>, lang: &str) -> bool {
    match lang.to_lowercase().as_str() {
        "js" | "javascript" | "typescript" | "ts" => {
            patterns.extend_from_slice(&[
                "",
                "# JavaScript/TypeScript specific",
                "/.npm/",
                "/.yarn/",
                "/jspm_packages/",
                "/bower_components/",
                "*.min.js",
                "*.min.css",
                "package-lock.json",
                "yarn.lock",
                "pnpm-lock.yaml",
                "/coverage/",
                "/.nyc_output/",
                "/.next/",
                "/.nuxt/",
                "/out/",
                "/.cache/",
                "/storybook-static/",
                ".eslintcache",
            ]);
            true
        },
        "py" | "python" => {
            patterns.extend_from_slice(&[
                "",
                "# Python specific",
                "/__pycache__/",
                "*.py[cod]",
                "*$py.class",
                "/.env/",
                "/.venv/",
                "/env/",
                "/venv/",
                "/ENV/",
                "/env.bak/",
                "/venv.bak/",
                "/.python-version",
                "/celerybeat-schedule",
                "/.pytest_cache/",
                "/.coverage",
                "/htmlcov/",
                "/.tox/",
                "/site/",
                "/.eggs/",
                "/.ipynb_checkpoints/",
                "*.egg-info/",
            ]);
            true
        },
        "rust" | "rs" => {
            patterns.extend_from_slice(&[
                "",
                "# Rust specific",
                "/target/",
                "Cargo.lock",
                "**/*.rs.bk",
                "**/*.pdb",
                "/benches/target/",
                "rust-toolchain.toml",
            ]);
            true
        },
        "go" | "golang" => {
            patterns.extend_from_slice(&[
                "",
                "# Go specific",
                "/bin/",
                "/pkg/",
                "*.test",
                "/vendor/",
                "/Godeps/",
                "/.glide/",
                "/.go-version",
                "go.work",
            ]);
            true
        },
        "c" | "cpp" | "c++" => {
            patterns.extend_from_slice(&[
                "",
                "# C/C++ specific",
                "*.d",
                "*.slo",
                "*.lo",
                "*.gch",
                "*.pch",
                "*.lai",
                "*.la",
                "/build*/",
                "/Debug/",
                "/Release/",
                "/compile_commands.json",
                "/.clangd/",
                "*.dSYM/",
                "/CMakeFiles/",
                "/CMakeScripts/",
                "/Testing/",
                "/cmake-build-*/",
                "CMakeCache.txt",
                "cmake_install.cmake",
            ]);
            true
        },
        "java" => {
            patterns.extend_from_slice(&[
                "",
                "# Java specific",
                "*.class",
                "*.jar",
                "*.war",
                "*.ear",
                "*.nar",
                "hs_err_pid*",
                "/target/",
                "/.mvn/",
                "/gradle/",
                "/.gradle/",
                "/out/",
                "/build/",
                "/.nb-gradle/",
                "/.settings/",
                "/.project",
                "/.classpath",
                "/.apt_generated/",
                "/.factorypath",
            ]);
            true
        },
        "cs" | "csharp" | "dotnet" => {
            patterns.extend_from_slice(&[
                "",
                "# C#/.NET specific",
                "/bin/",
                "/obj/",
                "*.user",
                "/_ReSharper*/",
                "*.ReSharper*",
                "*.DotSettings.user",
                "/packages/",
                "/.vs/",
                ".suo",
                "*.userosscache",
                "*.dbmdl",
                "*.jfm",
                "/Generated_Code/",
                "/_UpgradeReport_Files/",
                "/TestResults/",
                "*.[Pp]ublish.xml",
                "*.azurePubxml",
                ".builds",
                "*.pidb",
                "/project.lock.json",
                "/project.assets.json",
            ]);
            true
        },
        "php" => {
            patterns.extend_from_slice(&[
                "",
                "# PHP specific",
                "/vendor/",
                "/composer.lock",
                "/composer.phar",
                "/.phpunit.result.cache",
                "/.php_cs.cache",
                "/.php-cs-fixer.cache",
                "/.phpcs-cache",
                "/public/storage",
                "/public/hot",
                "/storage/*.key",
                "/public/build",
                "/build/logs/",
                "/.env.local",
                "/.env.*.local",
            ]);
            true
        },
        "ruby" | "rb" => {
            patterns.extend_from_slice(&[
                "",
                "# Ruby specific",
                "/.bundle/",
                "/vendor/bundle/",
                "/lib/bundler/man/",
                "*.gem",
                "/.rubocop-https?--*",
                "/.ruby-version",
                "/.ruby-gemset",
                "/.rvmrc",
                "/log/",
                "/tmp/",
                "/db/*.sqlite3",
                "/db/*.sqlite3-journal",
                "/public/system/",
                "/coverage/",
                "/spec/reports/",
            ]);
            true
        },
        "swift" => {
            patterns.extend_from_slice(&[
                "",
                "# Swift specific",
                "/.build/",
                "/Packages/",
                "/*.xcodeproj/",
                "/*.xcworkspace/",
                "/xcuserdata/",
                "/DerivedData/",
                "/.swiftpm/",
                "/Preview.html",
                "/timeline.xctimeline",
                "/playground.xcworkspace",
                "/*.hmap",
                "/*.ipa",
                "/*.dSYM.zip",
                "/*.dSYM",
            ]);
            true
        },
        "elixir" | "ex" => {
            patterns.extend_from_slice(&[
                "",
                "# Elixir specific",
                "/_build/",
                "/cover/",
                "/deps/",
                "/*.ez",
                "/erl_crash.dump",
                "/.elixir_ls/",
                "/.fetch",
                "/priv/static/",
            ]);
            true
        },
        _ => {
            println!("Warning: Unknown language '{}'. Using common patterns only.", lang);
            false
        }
    }
}

fn print_pattern_help() {
    println!("\nUnify Tool - Ignore Pattern Syntax Help\n");
    println!("The ignore patterns in .unify.toml follow gitignore syntax rules:\n");
    
    println!("Basic Rules:");
    println!("  - Lines starting with # are comments");
    println!("  - Blank lines are ignored");
    println!("  - Patterns ending with / match directories only");
    println!("  - Patterns starting with / match from the root of the project");
    println!("  - Patterns starting with ! negate a previous pattern (include instead of ignore)");
    println!("");
    
    println!("Wildcards:");
    println!("  * - Matches any sequence of characters except /");
    println!("  ? - Matches any single character except /");
    println!("  ** - Matches any sequence of directories (matches across directories)");
    println!("");
    
    println!("Examples:");
    println!("  *.log       - Ignore all .log files in any directory");
    println!("  /node_modules/ - Ignore node_modules directory at the project root only");
    println!("  build/      - Ignore all directories named 'build' anywhere in the project");
    println!("  /dist/**    - Ignore everything inside the 'dist' directory at the root");
    println!("  !important.log - Include important.log even if other patterns would ignore it");
    println!("");
    
    println!("Example Config File (.unify.toml):");
    println!("```");
    println!("ignores = [");
    println!("    # Root-level only patterns");
    println!("    \"/node_modules/\",");
    println!("    \"/build/\",");
    println!("");
    println!("    # Patterns that apply everywhere");
    println!("    \".git/\",");
    println!("    \"*.log\",");
    println!("");
    println!("    # Exceptions");
    println!("    \"!important.log\"");
    println!("]");
    println!("recursive = true");
    println!("header_template = \"\\n---\\nFile: {{{{relative_path}}}}\\n---\\n\"");
    println!("output = \"project.joined.txt\"");
    println!("```");
}