# Unify

A file unification tool that joins text files according to gitignore-style patterns. Useful for code reviews, documentation, LLM context windows, and sharing code.

## Install

```bash
cargo install --path .
```

With LLM token counting:
```bash
cargo install --path . --features tokens
```

## Quick Start

```bash
# Initialize a config file (auto-detects languages)
unify init

# Preview what would be included
unify --dry-run

# Unify all files
unify

# Unify specific directories
unify ./src ./lib

# Output as markdown with code blocks
unify --format markdown -o unified.md

# Use with a specific language preset
unify --language rust,js
```

## Configuration

Unify uses a layered config system. Each layer overrides the previous:

1. **Built-in defaults** (hardcoded)
2. **Global config** (`~/.config/unify/config.toml`)
3. **Project config** (`.unify.toml`, searched upward from working dir)
4. **Language presets** (embedded + customizable)
5. **CLI arguments** (highest priority)

### Example `.unify.toml`

```toml
language = ["rust"]

ignores = [
    ".git/",
    ".vscode/",
    ".idea/",
    "/.unify.toml",
]

# Only include specific paths (whitelist)
# includes = ["src/**", "Cargo.toml"]

# Append ignores without replacing
# extra_ignores = ["my-custom-dir/"]

recursive = true
follow_symlinks = false
skip_binary = true
max_file_size = "1MB"

format = "markdown"       # plain, markdown, xml, json
output = "unified.md"
wrap_code_block = true     # wrap files in fenced code blocks
code_block_lang = "auto"  # auto (uses file extension), none, or a specific lang
toc = true                # generate table of contents

header_template = "\n## {relative_path}\n\n"
footer_template = "\n"
```

### Template Variables

| Variable | Example | Description |
|----------|---------|-------------|
| `{relative_path}` | `src/main.rs` | Full path relative to project root |
| `{file_name}` | `main.rs` | File name only |
| `{extension}` | `rs` | File extension |
| `{lines}` | `142` | Line count |
| `{size}` | `4.2 KB` | Human-readable size |
| `{dir}` | `src` | Directory portion |

### Output Formats

| Format | Description | Default output |
|--------|-------------|---------------|
| `plain` | Simple text with comment headers | `unified.txt` |
| `markdown` | Headings + fenced code blocks | `unified.md` |
| `xml` | Structured XML with CDATA | `unified.xml` |
| `json` | Array of file objects | `unified.json` |

## Language Presets

Presets provide language-specific ignore patterns. They're loaded from embedded defaults or your custom overrides.

```bash
# List available presets
unify preset list

# Show a preset's contents
unify preset show rust

# Export for customization
unify preset export rust
# Edit ~/.config/unify/languages/rust.toml

# Reset to built-in
unify preset reset rust
```

Available: `rust`, `python`, `javascript`, `go`, `java`, `c`, `csharp`, `php`, `ruby`, `swift`, `elixir`

Aliases: `rs`→rust, `py`→python, `js`/`ts`→javascript, `cpp`→c, `cs`→csharp, `rb`→ruby, `ex`→elixir

Languages are auto-detected from project files (Cargo.toml, package.json, etc.) when not specified.

## Global Config

Set defaults that apply to all projects:

```bash
unify init --global
# Edit ~/.config/unify/config.toml
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `UNIFY_OUTPUT` | Default output file path |

## Pattern Syntax

Patterns follow gitignore rules:

- `*.log` — ignore all .log files anywhere
- `/target/` — ignore target directory at project root only
- `build/` — ignore all directories named 'build'
- `!important.log` — re-include despite other patterns
- `**/*.min.js` — match across directory boundaries

Run `unify help` for full pattern syntax documentation.

## License

MIT
