# Context-Only Mode Testing

This document tests the new context-only mode feature where messages containing only @file references add context but don't make API calls.

## Test Cases

### 1. Single File Reference

```bash
flexorama "@Cargo.toml"
```

**Expected behavior:**
- Adds Cargo.toml to context
- Shows "✓ Added context file: Cargo.toml"
- No API call made
- No response printed
- Ready for next input

### 2. Multiple File References

```bash
flexorama "@src/main.rs @src/lib.rs @README.md"
```

**Expected behavior:**
- Adds all three files to context
- Shows success messages for each file
- No API call made
- No response printed

### 3. File Reference with Question

```bash
flexorama "@Cargo.toml What is this project about?"
```

**Expected behavior:**
- Adds Cargo.toml to context
- Makes API call with the question
- Shows AI response

### 4. Mixed Content

```bash
flexorama "@file1.txt Some text here @file2.txt"
```

**Expected behavior:**
- Adds both files to context
- Makes API call with "Some text here"
- Shows AI response

### 5. File Reference with Only Whitespace

```bash
flexorama "@file1.txt    "
```

**Expected behavior:**
- Adds file1.txt to context
- No API call made (whitespace-only after cleaning)

### 6. Invalid File Reference

```bash
flexorama "@nonexistent.txt"
```

**Expected behavior:**
- Shows error message for failed file addition
- No API call made

### 7. Mixed Valid and Invalid Files

```bash
flexorama "@valid.txt @nonexistent.txt"
```

**Expected behavior:**
- Successfully adds valid.txt
- Shows error for nonexistent.txt
- No API call made

## Interactive Mode Testing

### Context Building Workflow

```bash
# Start interactive mode
flexorama

# Add context files
> @src/main.rs
✓ Added context file: src/main.rs

> @config.json
✓ Added context file: config.json

> @README.md
✓ Added context file: README.md

# Now ask a question about the loaded context
> Explain the project structure
[AI response about the project]
```

### Context Verification

```bash
# Add files and check context
> @Cargo.toml @src/main.rs
✓ Added context file: Cargo.toml
✓ Added context file: src/main.rs

> /context
📝 Current Conversation Context
──────────────────────────────────────────────────
[1] USER (1 content blocks)
  └─ Block 1: Text
Context from file '/path/to/Cargo.toml':
[2] USER (1 content blocks)  
  └─ Block 1: Text
Context from file '/path/to/src/main.rs':
──────────────────────────────────────────────────
```

## Non-Interactive Mode Testing

### Context Only

```bash
echo "@file1.txt @file2.txt" | flexorama --non-interactive
```

**Expected behavior:**
- Adds both files to context
- No API call
- No output

### Context with Question

```bash
echo "@file1.txt @file2.txt What do these files do?" | flexorama --non-interactive
```

**Expected behavior:**
- Adds both files to context
- Makes API call with the question
- Shows AI response

## Single Message Mode Testing

### Context Only

```bash
flexorama -m "@file1.txt @file2.txt"
```

**Expected behavior:**
- Adds both files to context
- No API call
- No output

### Context with Question

```bash
flexorama -m "@file1.txt @file2.txt Explain the difference"
```

**Expected behavior:**
- Adds both files to context
- Makes API call with the question
- Shows AI response

## Edge Cases

### 1. Empty @file syntax

```bash
flexorama "@"
```

**Expected behavior:**
- No file added (invalid syntax)
- No API call made

### 2. @file at end with punctuation

```bash
flexorama "@file.txt."
```

**Expected behavior:**
- Tries to add "file.txt." (including the dot)
- May fail if file doesn't exist with that exact name

### 3. Multiple @ symbols

```bash
flexorama "@@file.txt"
```

**Expected behavior:**
- Tries to add "@file.txt" (including the first @)
- May fail if file doesn't exist with that exact name

### 4. File paths with spaces

```bash
flexorama "@\"file with spaces.txt\""
```

**Expected behavior:**
- Should handle quoted file paths
- Adds the file if it exists

## Performance Benefits

### Before (Old Behavior)

```bash
flexorama "@file1.txt @file2.txt"
# Would:
# 1. Add both files to context
# 2. Make empty API call
# 3. Receive empty response
# 4. Waste tokens and time
```

### After (New Behavior)

```bash
flexorama "@file1.txt @file2.txt"
# Now:
# 1. Add both files to context
# 2. Detect no actual content
# 3. Skip API call
# 4. Save tokens and time
```

## Token Savings

This feature saves tokens by avoiding unnecessary API calls when users are just building context. Typical savings:

- **Context building sessions**: 100-1000+ tokens saved
- **File loading workflows**: 50-200 tokens saved per context-only message
- **Interactive context preparation**: Significant cumulative savings

## Usage Patterns

### Pattern 1: Context Loading then Questions

```bash
# Load context
flexorama "@config.json @src/main.rs @README.md"

# Ask questions
flexorama "How does this application work?"
flexorama "What are the main dependencies?"
flexorama "Explain the architecture"
```

### Pattern 2: Incremental Context Building

```bash
# Interactive mode
flexorama
> @package.json
✓ Added context file: package.json
> @src/index.js
✓ Added context file: src/index.js
> @styles/main.css
✓ Added context file: styles/main.css
> Describe this web application
[AI response]
```

### Pattern 3: Context Switching

```bash
# Load project A context
flexorama "@projectA/config.json @projectA/main.js"
# Ask questions about project A

# Clear context (start new session)
flexorama
> @projectB/config.py @projectB/app.py
# Ask questions about project B
```

This context-only mode provides a more efficient workflow for building context before asking questions, saving both time and API costs.