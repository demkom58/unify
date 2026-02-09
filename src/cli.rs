use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "A file unification tool that joins text files according to gitignore-style patterns",
    long_about = "Unify reads text files from your project directory and combines them into a single output.\n\
                  It uses gitignore-style patterns to determine which files to include or exclude.\n\
                  Useful for code reviews, documentation, LLM context, and sharing code.",
    after_help = "EXAMPLES:\n\
    \x20 unify init --language rust\n\
    \x20 unify\n\
    \x20 unify ./src ./lib\n\
    \x20 unify -o combined.md --language rust,js\n\
    \x20 unify --dry-run\n\
    \x20 unify preset list\n\
    \x20 unify preset export rust"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Output file path (overrides config)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Path to config file
    #[arg(short, long, default_value = ".unify.toml")]
    pub config: PathBuf,

    /// Working directory
    #[arg(short, long)]
    pub dir: Option<PathBuf>,

    /// Languages to apply presets for (comma-separated)
    #[arg(short, long, value_delimiter = ',')]
    pub language: Option<Vec<String>>,

    /// Maximum file size to include (e.g. "512KB", "2MB")
    #[arg(long)]
    pub max_size: Option<String>,

    /// Follow symbolic links
    #[arg(long)]
    pub follow_symlinks: bool,

    /// Output format: plain, markdown, xml, json
    #[arg(long)]
    pub format: Option<String>,

    /// Preview what would be unified without producing output
    #[arg(long)]
    pub dry_run: bool,

    /// Specific directories/files to process
    #[arg()]
    pub paths: Vec<PathBuf>,
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new .unify.toml config file
    Init {
        /// Programming languages (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        language: Option<Vec<String>>,

        /// Force overwrite existing config
        #[arg(short, long)]
        force: bool,

        /// Initialize global config instead of project config
        #[arg(long)]
        global: bool,
    },

    /// Manage language presets
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },

    /// Show pattern syntax help
    Help,
}

#[derive(clap::Subcommand, Debug)]
pub enum PresetAction {
    /// List all available presets (built-in + custom)
    List,

    /// Show contents of a preset
    Show {
        /// Preset name (e.g. "rust", "python")
        name: String,
    },

    /// Export a built-in preset for customization
    Export {
        /// Preset name
        name: String,

        /// Overwrite existing custom preset
        #[arg(long)]
        force: bool,
    },

    /// Delete custom preset, reverting to built-in
    Reset {
        /// Preset name
        name: String,
    },
}
