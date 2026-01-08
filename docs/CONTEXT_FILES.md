# Context Files Feature

## Overview
The Flexorama now supports adding files as context to provide additional information to the AI during conversations.

## Usage

### Command Line Options
- `-f <FILE>` or `--file <FILE>`: Specify files to include as context
- Multiple files can be specified: `-f file1.md -f file2.txt`

### Automatic Context
- `AGENTS.md` is automatically included as context if it exists in the current directory

### Examples

```bash
# Add specific files as context
flexorama -f README.md -f config.toml -m "Explain the project structure"

# AGENTS.md will be automatically added if it exists
flexorama -m "What agents are available?"

# Multiple context files
flexorama -f docs/api.md -f examples/sample.rs -m "Show me how to use the API"
```

## Implementation Details

### Main Changes
1. **CLI Arguments**: Added `-f/--file` option to specify context files
2. **Agent Context**: Added `add_context_file()` method to include file contents
3. **Auto-detection**: Automatically includes `AGENTS.md` if present
4. **Error Handling**: Gracefully handles missing or unreadable files

### Code Structure
- `main.rs`: Added CLI argument parsing and context file loading
- `agent.rs`: Added context file handling functionality
- Context files are added to the conversation before user messages

### File Processing
- Files are read and formatted as context messages
- Content is wrapped in code blocks for better AI understanding
- File paths are resolved using shell expansion and absolute paths

## Benefits
- Provides AI with relevant background information
- Reduces need to repeatedly describe project structure
- Enables more contextually accurate responses
- Supports project documentation and configuration files