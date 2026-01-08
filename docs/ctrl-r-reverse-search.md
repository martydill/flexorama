# Ctrl+R Reverse Search Feature

## Overview

The Ctrl+R reverse search feature provides readline-like reverse incremental search functionality for the Flexorama interactive mode. This allows you to quickly search through your command history by typing a substring of a previous command.

## How to Use

### Starting Reverse Search

1. **Press Ctrl+R** to start reverse search mode
2. The prompt will change to: `(reverse-i-search)`' 
3. Type characters to search for matching commands in your history

### Navigation

- **Ctrl+R** (or just 'r') - Find next (more recent) match
- **↑ Arrow** - Find previous (older) match  
- **↓ Arrow** - Find next (more recent) match
- **Enter** - Accept the current match and return to normal input
- **ESC** - Cancel search and restore your original input
- **Backspace** - Remove last character from search query
- **Type characters** - Add to search query

### Search Behavior

- **Case-insensitive**: Search ignores case differences
- **Substring matching**: Finds commands containing your search text anywhere
- **Reverse chronological**: Shows most recent matches first
- **Real-time highlighting**: Search query is highlighted within matched commands
- **File highlighting**: Maintains @file syntax highlighting in matches

## Examples

### Basic Search
```
> (reverse-i-search)`git': git status
```
This shows the most recent command containing "git".

### Multiple Matches
```
> (reverse-i-search)`cargo': cargo test --release
```
Press Ctrl+R again to cycle to:
```
> (reverse-i-search)`cargo': cargo build
```

### Finding Commands with @file References
```
> (reverse-i-search)`main.rs': explain @src/main.rs
```
The @file reference will be highlighted in blue.

## Integration with Existing Features

The reverse search feature integrates seamlessly with existing Flexorama features:

- **History Navigation**: Works alongside up/down arrow history navigation
- **File Highlighting**: Maintains @file syntax highlighting in search results
- **Autocomplete**: Available when you exit reverse search mode
- **Multiline Input**: Can search for and select multiline commands
- **Cancellation**: ESC works both for canceling search and AI conversations

## Technical Implementation

### Architecture

The feature is built using several key components:

1. **ReverseSearchState**: Manages search state including query, matches, and current position
2. **InputHistory**: Extended with reverse search methods and state management
3. **Event Handling**: Special handling for Ctrl+R and other keys during search mode
4. **Display Rendering**: Custom prompt rendering with highlighting

### Key Methods

- `start_reverse_search()`: Initialize reverse search mode
- `update_reverse_search()`: Update search query and find matches
- `reverse_search_next()`: Navigate to next match
- `reverse_search_prev()`: Navigate to previous match
- `finish_reverse_search()`: Accept current match and exit search mode
- `cancel_reverse_search()`: Cancel search and restore original input

### Performance Considerations

- **Efficient Search**: Uses case-insensitive substring matching
- **History Limits**: Maintains 1000 most recent commands to prevent memory issues
- **Lazy Evaluation**: Searches only when query changes
- **Optimized Rendering**: Fast redraw for search navigation

## User Experience

### Visual Feedback

- **Search Prompt**: Clear `(reverse-i-search)`'` indicator
- **Query Highlighting**: Search query highlighted in yellow/bold
- **Match Highlighting**: Query text highlighted within matches
- **Failed Search**: Shows "(failed)" when no matches found

### Error Handling

- **Empty History**: Gracefully handles when no history exists
- **No Matches**: Shows "(failed)" indicator
- **Cancellation**: Clean restoration of original input
- **State Management**: Proper cleanup when exiting search mode

## Compatibility

- **Cross-platform**: Works on Windows, macOS, and Linux
- **Terminal Compatibility**: Compatible with most modern terminals
- **Input Methods**: Works with various keyboard layouts and input methods
- **Accessibility**: Maintains screen reader compatibility

## Future Enhancements

Potential improvements for future versions:

1. **Regular Expression Search**: Support for regex patterns
2. **Fuzzy Matching**: Implement fuzzy search algorithms
3. **Search Filters**: Filter by command type (shell commands, AI queries, etc.)
4. **Persistent History**: Save history across sessions
5. **Search Statistics**: Show match count and position
6. **Custom Keybindings**: Allow user-configurable search keys

## Troubleshooting

### Common Issues

1. **Ctrl+R Not Working**: Ensure terminal supports Ctrl+R and no other application is intercepting it
2. **No Search Results**: Check that history contains commands and they're not empty
3. **Strange Characters**: Verify terminal encoding is UTF-8 compatible
4. **Performance Issues**: Consider reducing history size if searching is slow

### Debug Information

The feature includes comprehensive logging that can be enabled with:
```bash
RUST_LOG=debug flexorama
```

This will show detailed information about search operations and state changes.