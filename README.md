# Unify - Text File Unification Tool

![GitHub release (latest by date)](https://img.shields.io/github/v/release/demkom58/unify)
![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/demkom58/unify/release.yml)
![License](https://img.shields.io/github/license/demkom58/unify)

Unify is a powerful CLI tool that joins text files from your project into a single, unified document. It uses gitignore-style pattern matching to determine which files to include or exclude, making it perfect for code reviews, documentation, and sharing code snippets.

## Features

- **Gitignore-style patterns**: Familiar syntax for including/excluding files
- **Multiple language support**: Built-in configurations for popular programming languages
- **Custom headers**: Format file headers with customizable templates
- **Binary file detection**: Automatically skips binary files
- **Recursive traversal**: Process entire directory trees
- **Path filtering**: Process only specific directories within a project
- **Flexible output**: Write to file or stdout

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/demkom58/unify.git
cd unify

# Build and install
cargo build --release
cargo install --path .
```

### From Cargo

```bash
cargo install unify
```

### Pre-built Binaries

Download the latest binary for your platform from the [Releases page](https://github.com/demkom58/unify/releases).

## Quick Start

```bash
# Create a default configuration file
unify init

# Create a configuration for specific languages
unify init --language rust,js,python

# Join files according to the configuration
unify

# Join only files in specific directories
unify ./src ./lib

# Specify output file
unify -o combined.txt
```

## Configuration

Unify uses a `.unify.toml` file in your project directory for configuration:

```toml
ignores = [
    # Root-level only patterns
    "/node_modules/",
    "/build/",
    "/dist/",
    
    # Patterns that apply everywhere
    ".git/",
    "*.log",
    
    # Exceptions
    "!important.log"
]
recursive = true
header_template = "\n---\nFile: {relative_path}\n---\n"
output = "project.joined.txt"
```

### Configuration Options

| Option | Description | Default |
|--------|-------------|---------|
| `ignores` | Array of gitignore-style patterns | `[]` |
| `recursive` | Whether to process subdirectories | `true` |
| `header_template` | Template for file headers | `"// File: {relative_path}\n"` |
| `output` | Output file name (use "-" for stdout) | `"unified.txt"` |

### Header Template Variables

- `{relative_path}` - Path relative to the working directory
- `{file_name}` - Name of the file without the path

## Usage

```
USAGE:
    unify [OPTIONS] [PATHS]...
    unify <SUBCOMMAND>

ARGS:
    <PATHS>...    Specific directories to process (if not specified, process the whole working directory)

OPTIONS:
    -c, --config <CONFIG>    Path to the configuration file [default: .unify.toml]
    -d, --dir <DIR>          Directory to process (defaults to current directory)
    -h, --help               Print help information
    -o, --output <OUTPUT>    Path where the unified output will be written
    -V, --version            Print version information

SUBCOMMANDS:
    help    Show help about pattern syntax
    init    Initialize a new .unify.toml config file
```

### Examples

```bash
# Process all files in current directory using .unify.toml
unify

# Process only specific directories
unify ./src ./include

# Use a custom configuration file
unify -c custom-config.toml

# Specify working directory and output file
unify -d /path/to/project -o unified.txt

# Show pattern syntax help
unify help
```

## Pattern Syntax

Unify uses gitignore-style patterns:

- Lines starting with `#` are comments
- Patterns ending with `/` match directories only
- Patterns starting with `/` match from the root directory
- Patterns starting with `!` negate a previous pattern (include)
- `*` matches any sequence of characters except `/`
- `**` matches any sequence of directories

Examples:
- `*.log` - Ignore all .log files anywhere
- `/node_modules/` - Ignore node_modules at the root only
- `build/` - Ignore all build directories anywhere
- `!important.log` - Include important.log even if ignored by other patterns

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request