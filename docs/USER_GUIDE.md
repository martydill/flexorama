# Flexorama User Guide

This guide provides comprehensive documentation for using the Flexorama tool, a powerful terminal-based AI assistant that supports multiple interaction modes, file operations, and advanced features.

## Table of Contents

1. [Installation](#installation)
2. [Quick Start](#quick-start)
3. [Configuration](#configuration)
4. [Usage Modes](#usage-modes)
5. [File Operations](#file-operations)
6. [Interactive Commands](#interactive-commands)
7. [Advanced Features](#advanced-features)
8. [Troubleshooting](#troubleshooting)

## Installation

### Prerequisites

- Node.js (version 14 or higher)
- npm (usually comes with Node.js)

### Installation Steps

```bash
# Clone the repository
git clone https://github.com/your-repo/flexorama.git
cd flexorama

# Install dependencies
npm install

# Build the project
npm run build

# Install globally (optional)
npm install -g .
```

## Quick Start

### Basic Usage

```bash
# Start interactive mode
flexorama

# Send a single message
flexorama -m "Hello, how are you?"

# Get help
flexorama --help
```

## Configuration

### Environment Variables

Set these environment variables to configure the agent:

```bash
export ANTHROPIC_AUTH_TOKEN="your-api-key"
export ANTHROPIC_BASE_URL="https://api.anthropic.com/v1"
```

### Configuration File

Create a configuration file at `~/.config/flexorama/config.toml`:

```toml
base_url = "https://api.anthropic.com/v1"
default_model = "glm-4.6"
max_tokens = 4096
temperature = 0.7
```

⚠️ **IMPORTANT**: API keys are **never** stored in configuration files for security reasons. Use environment variables or command line flags.

### Command Line Options

| Option | Short | Description |
|--------|-------|-------------|
| `--message` | `-m` | Single message mode |
| `--file` | `-f` | Include file as context |
| `--api-key` | `-k` | API key override |
| `--base-url` | `-b` | Base URL override |
| `--model` | | Model selection |
| `--max-tokens` | | Maximum tokens |
| `--temperature` | | Temperature setting |
| `--non-interactive` | | Read from stdin |
| `--help` | `-h` | Show help |
| `--version` | `-v` | Show version |

## Usage Modes

### 1. Interactive Mode

Start a conversation with the Flexorama:

```bash
flexorama
```

Features:
- Maintains conversation history
- Supports slash commands
- Real-time response

### 2. Single Message Mode

Send one message and get a response:

```bash
flexorama -m "Explain quantum computing"
```

### 3. Non-Interactive Mode

Read from stdin for scripting:

```bash
echo "Help me understand this code" | flexorama --non-interactive
```

### 4. Context-Aware Mode

Include files as context:

```bash
# Using command line flags
flexorama -f config.toml -f package.json "Explain this project"

# Using @file syntax
flexorama "What does @Cargo.toml contain?"
```

## File Operations

### Including Files

The agent supports multiple ways to include files:

#### Method 1: Command Line Flags

```bash
flexorama -f file1.txt -f file2.rs "Analyze these files"
```

#### Method 2: @file Syntax (Recommended)

```bash
# Single file
flexorama "What does @config.json contain?"

# Multiple files
flexorama "Compare @src/main.rs and @src/lib.rs"

# File with specific question
flexorama "Explain the authentication logic in @auth.js"

# Multiple files without question
flexorama "@package.json @README.md"
```

#### Method 3: Auto-inclusion

`AGENTS.md` is automatically included if it exists in the current directory.

### File Path Handling

- **Absolute paths**: `/home/user/document.txt`
- **Relative paths**: `./src/main.rs` or `src/main.rs`
- **Home directory**: `~/config/settings.toml`
- **Wildcards**: Not currently supported

### Supported File Types

The agent can read any text-based file:
- Source code (.js, .py, .rs, .java, etc.)
- Configuration files (.json, .toml, .yaml, .xml)
- Documentation (.md, .txt, .rst)
- Web files (.html, .css, .scss)

## Interactive Commands

When in interactive mode, use these slash commands:

### Help and Information

```bash
/help          # Show all available commands
/stats         # Show token usage statistics
/usage         # Alias for /stats
/context       # Show current conversation context
```

### Control Commands

```bash
/reset-stats   # Reset token usage statistics
/exit          # Exit the program
/quit          # Alias for /exit
```

## Advanced Features

### Tool Support

The agent can execute various tools for file operations:

#### Available Tools

1. **read_file**: Read file contents
2. **write_file**: Write content to files
3. **edit_file**: Replace specific text in files
4. **list_directory**: List directory contents
5. **create_directory**: Create directories
6. **delete_file**: Delete files or directories

#### Tool Usage Examples

```bash
# Ask the agent to perform file operations
flexorama "Read the contents of @src/config.js and create a backup"

# Multi-step operations
flexorama "List all .rs files in src/ directory, then read main.rs"
```

### Context Management

The agent maintains conversation history and can:

- Reference previous messages
- Remember file contents from earlier in the conversation
- Build upon previous responses

### Error Handling

The agent provides clear error messages for:

- **Authentication failures**: Invalid API keys
- **Network issues**: Connection problems
- **File errors**: Missing files, permission issues
- **Tool failures**: Invalid operations
- **Syntax errors**: Invalid @file references

## Troubleshooting

### Common Issues

#### 1. Authentication Error

**Problem**: `API authentication failed`

**Solution**:
```bash
# Check environment variable
echo $ANTHROPIC_AUTH_TOKEN

# Or set it temporarily
export ANTHROPIC_AUTH_TOKEN="your-api-key"

# Or use command line
flexorama -k "your-api-key" -m "test"
```

#### 2. File Not Found

**Problem**: `File not found: @nonexistent.txt`

**Solution**:
- Check file path spelling
- Use absolute or relative paths correctly
- Verify file exists and is readable

#### 3. Permission Denied

**Problem**: `Permission denied` when reading files

**Solution**:
- Check file permissions
- Use sudo if necessary (not recommended)
- Move file to accessible location

#### 4. Network Issues

**Problem**: `Connection timeout` or `Network error`

**Solution**:
- Check internet connection
- Verify API endpoint is accessible
- Try using a different base URL if behind firewall

### Debug Mode

Enable debug output for troubleshooting:

```bash
DEBUG=1 flexorama -m "test message"
```

### Getting Help

If you encounter issues:

1. Check this guide first
2. Run `flexorama --help` for command options
3. Use `/help` in interactive mode
4. Check the GitHub issues page
5. Create a new issue with details about your problem

## Tips and Best Practices

### Efficient Usage

1. **Use @file syntax** for cleaner commands
2. **Group related files** in single requests
3. **Use specific questions** with file references
4. **Leverage interactive mode** for complex tasks

### Performance Optimization

1. **Include only necessary files** to reduce token usage
2. **Use /reset-stats** to monitor token consumption
3. **Break large files** into smaller chunks if needed
4. **Use non-interactive mode** for automation scripts

### Security Considerations

1. **API Key Security**:
   - **Never store API keys** in configuration files
   - **Use environment variables** for API keys (recommended)
   - **Use command line flags** for temporary API keys
   - **API keys are automatically excluded** from config files
   - **Never commit API keys** to version control

2. **File Safety**:
   - Be careful with file paths to avoid exposing sensitive data
   - Review auto-included files before sharing conversations
   - Use .env files for local development (add to .gitignore)

3. **Secure API Key Usage Examples**:

```bash
# ✅ RECOMMENDED: Environment variable
export ANTHROPIC_AUTH_TOKEN="your-api-key"
flexorama -m "Hello"

# ✅ SECURE: Command line flag
flexorama -k "your-api-key" -m "Hello"

# ✅ DEVELOPMENT: .env file (add to .gitignore)
echo 'ANTHROPIC_AUTH_TOKEN="your-api-key"' > .env
source .env && flexorama -m "Hello"
```

4. ❌ **NEVER DO**:
```toml
# ❌ NEVER store API key in config.toml
api_key = "sk-ant-api03-..."  # SECURITY RISK!
```

## Examples

### Code Analysis

```bash
# Analyze a Rust project
flexorama "@Cargo.toml @src/main.rs @src/lib.rs Explain this project structure"

# Review Python code
flexorama "Find potential bugs in @app.py and suggest improvements"
```

### Documentation Generation

```bash
# Generate README
flexorama "@src/index.js @package.json Create a README for this project"

# Document API endpoints
flexorama "@api/routes.js @api/models.js Document all API endpoints"
```

### Configuration Management

```bash
# Compare configurations
flexorama "Compare @dev.config.json and @prod.config.json"

# Generate config template
flexorama "@config.example.json Create a template configuration file"
```

## Conclusion

The Flexorama is a powerful tool that can significantly enhance your development workflow. By mastering its features and following best practices, you can leverage AI assistance for code analysis, documentation, configuration management, and much more.

For the most up-to-date information, check the project documentation and GitHub repository.