# @file Syntax Context Support

This document describes the new `@file` syntax support for automatically attaching file context to Flexorama messages.

## Overview

The Flexorama now supports a convenient `@path-to-file` syntax that allows you to automatically include file content as context in your messages. This eliminates the need to manually specify files using the `-f` or `--file` flags when you want to reference files in your conversation.

## Syntax

Use `@` followed by the file path anywhere in your message:

```bash
flexorama "What does @Cargo.toml contain?"
flexorama "Compare @file1.rs and @file2.rs"
flexorama "Explain the configuration in @config.toml"
```

## Features

### Automatic File Detection
- The agent scans messages for `@filename` patterns
- Extracts file paths and automatically adds them as context
- Supports multiple file references in a single message

### Message Cleaning
- Removes `@file` syntax from the message before processing
- Preserves your original question/request
- Handles cases where message contains only file references

### Error Handling
- Gracefully handles non-existent files
- Shows clear error messages for invalid file paths
- Continues processing with valid files when mixed valid/invalid files are provided

### Path Resolution
- Supports relative paths: `@src/main.rs`
- Supports absolute paths: `@/home/user/config.toml`
- Supports tilde expansion: `@~/Documents/file.txt`
- Supports paths with spaces (when quoted): `@"My Documents/file.txt"`

## Usage Examples

### Single File Reference
```bash
flexorama "What does @Cargo.toml contain?"
```

### Multiple File References
```bash
flexorama "Compare @src/main.rs and @src/lib.rs"
```

### File Reference with Question
```bash
flexorama "Can you explain the Rust code in @src/main.rs?"
```

### Only File References
```bash
flexorama "@file1.txt @file2.txt @file3.txt"
```

### Mixed Valid and Invalid Files
```bash
flexorama "Analyze @config.toml and @nonexistent.txt"
```

## Integration with Existing Features

### Compatibility with `-f` Flag
The `@file` syntax works alongside the existing `-f`/`--file` flag:

```bash
flexorama -f base.txt "Compare @file1.txt with the base file"
```

### Auto-inclusion of AGENTS.md
The automatic inclusion of `AGENTS.md` continues to work as before.

### Context Management
Files added via `@file` syntax appear in the conversation context and can be viewed using `/context` command.

## Error Messages

### File Not Found
```
✗ Failed to add context file 'nonexistent.txt': No such file or directory (os error 2)
```

### Invalid Path
```
✗ Failed to add context file 'invalid@path': Failed to read file '...': ...
```

## Implementation Details

### Regex Pattern
The system uses the regex pattern `@([^\s@]+)` to match file references:
- Matches `@` followed by any non-whitespace, non-`@` characters
- Stops at whitespace or another `@` symbol
- Handles paths with dots, slashes, and other common characters

### Processing Order
1. Extract file paths from message using regex
2. Add each file as context (with error handling)
3. Remove `@file` syntax from message
4. Add cleaned message to conversation (if not empty)
5. Process the conversation normally

### Token Usage
Files added via `@file` syntax count towards token usage just like manually added context files.

## Testing

A test script is provided at `test_context_syntax.sh` that creates sample files and provides test commands:

```bash
# Run the test script
./test_context_syntax.sh

# Test individual commands
flexorama "What does @test_file1.txt contain?"
flexorama "Compare @test_file1.txt and @test_file2.txt"
flexorama "@test_file1.txt @test_file2.txt"
```

## Limitations

### Current Limitations
- File paths with spaces require quoting: `@"My File.txt"`
- Nested directory paths work but may need proper escaping
- No support for glob patterns or wildcards
- File content is read at message processing time (not cached)

### Future Enhancements
- Support for glob patterns: `@src/*.rs`
- File content caching for repeated references
- Support for directory traversal with `@dir/`
- Integration with git for file versioning

## Comparison with Existing Methods

| Method | Syntax | Pros | Cons |
|--------|--------|------|------|
| `-f` flag | `flexorama -f file.txt "question"` | Explicit, clear separation | Requires separate flag |
| `@file` syntax | `flexorama "What's in @file.txt?"` | Natural, inline usage | Mixed with message content |
| stdin piping | `cat file.txt \| flexorama` | Works with any tool | Less convenient for multiple files |

The `@file` syntax provides the most natural and convenient way to reference files directly within your messages.