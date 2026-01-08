# ESC Key Cancellation Feature

## Overview

The Flexorama agent now supports cancelling ongoing AI conversations using the ESC key. This feature provides users with immediate control over long-running AI responses and tool executions.

## How It Works

### Cancellation Mechanism

1. **ESC Detection**: The application monitors keyboard input during AI conversations
2. **Cancellation Flag**: A shared atomic boolean flag signals cancellation across async tasks
3. **Graceful Termination**: API calls, streaming responses, and tool executions check for cancellation
4. **User Feedback**: Clear visual feedback shows when cancellation is triggered

### Technical Implementation

- **Atomic Flag**: Uses `Arc<AtomicBool>` for thread-safe cancellation signaling
- **Background Listener**: Tokio task monitors ESC key presses during conversations
- **Cancellation Points**: Checks for cancellation at multiple points:
  - Before API calls
  - During streaming response processing
  - Before tool execution
  - Between tool execution iterations

## Usage

### Interactive Mode

```bash
# Start interactive mode
flexorama

# Send a message that might take a while
> Write a comprehensive analysis of machine learning algorithms

# Press ESC during processing to cancel
# Output: 🛑 Cancelling AI conversation...

# Continue with a new request
> What's the weather like today?
```

### Single Message Mode

```bash
# Start a long-running request
flexorama -m "Generate a complete Rust application with multiple modules"

# Press ESC during processing to cancel
# Output: 🛑 AI conversation cancelled by user
```

### Streaming Mode

```bash
# Start streaming response
flexorama --stream -m "Explain quantum computing in detail"

# Press ESC during streaming to cancel
# Output: 🛑 Cancelling AI conversation...
```

## Behavior

### What Gets Cancelled

- **API Calls**: In-progress requests to the AI model
- **Streaming Responses**: Real-time response generation
- **Tool Execution**: Any tools being executed by the AI
- **Multi-turn Conversations**: Ongoing conversation loops

### What Is Preserved

- **Conversation History**: Previous messages and context remain intact
- **System Prompts**: Current system prompt settings are maintained
- **Context Files**: Any loaded context files remain available
- **Configuration**: All current settings are preserved

### User Experience

- **Immediate Response**: Cancellation happens instantly upon ESC press
- **Visual Feedback**: Clear message shows cancellation status
- **Clean State**: Returns to prompt without corruption
- **Continued Operation**: Can immediately start new conversations

## Error Handling

### Cancellation Errors

When a conversation is cancelled, the system:

1. **Detects Cancellation**: Checks the atomic flag at cancellation points
2. **Propagates Cancellation**: Returns "CANCELLED" error up the call chain
3. **Displays Feedback**: Shows user-friendly cancellation message
4. **Maintains State**: Preserves conversation context and settings

### Error Messages

```
🛑 AI conversation cancelled by user
```

## Implementation Details

### Key Components

#### Main.rs Changes

- **ESC Key Handler**: Added ESC detection in input loop
- **Cancellation Flag**: Shared flag for signaling cancellation
- **Background Task**: Tokio task monitors ESC during conversations
- **Error Handling**: Proper handling of cancellation errors

#### Agent.rs Changes

- **Cancellation Parameter**: Added cancellation flag to processing methods
- **Cancellation Checks**: Checks for flag during conversation processing
- **Tool Execution**: Checks before executing each tool

#### Anthropic.rs Changes

- **Streaming Cancellation**: Checks during streaming response processing
- **API Call Cancellation**: Checks before making API requests
- **Chunk Processing**: Checks during stream chunk processing

### Thread Safety

- **Atomic Operations**: Uses `AtomicBool` with `SeqCst` ordering
- **Arc Sharing**: Shared ownership across async tasks
- **Race Conditions**: Proper synchronization prevents data races

### Performance Considerations

- **Low Overhead**: Minimal performance impact from cancellation checks
- **Efficient Monitoring**: Background task uses efficient polling
- **Graceful Degradation**: No impact when cancellation isn't used

## Limitations

### Current Limitations

1. **Input Mode**: ESC during input typing cancels input entry, not conversation
2. **Shell Commands**: Direct `!` commands cannot be cancelled with ESC
3. **Network Timeouts**: Cancellation may wait for network timeouts in some cases

### Future Enhancements

- **Shell Command Cancellation**: Extend ESC to cancel `!` commands
- **Input Cancellation**: Allow ESC during input to cancel typing
- **Partial Response Saving**: Option to save partial responses before cancellation

## Security Considerations

- **No State Corruption**: Cancellation maintains application state integrity
- **Resource Cleanup**: Proper cleanup of resources on cancellation
- **Atomic Operations**: Thread-safe cancellation signaling

## Testing

### Test Scenarios

1. **Basic Cancellation**: Cancel simple conversation
2. **Streaming Cancellation**: Cancel during streaming response
3. **Tool Execution Cancellation**: Cancel during tool execution
4. **Multi-turn Cancellation**: Cancel during conversation loops
5. **State Preservation**: Verify conversation history is maintained

### Manual Testing

```bash
# Test basic cancellation
flexorama
> Tell me a long story
[Press ESC]
# Verify cancellation message and continued operation

# Test streaming cancellation
flexorama --stream
> Explain quantum physics
[Press ESC during streaming]
# Verify immediate cancellation

# Test tool execution cancellation
> Create a large file with 1000 lines of text
[Press ESC during file creation]
# Verify tool execution is cancelled
```

## Conclusion

The ESC key cancellation feature provides users with immediate control over AI conversations, improving the user experience by allowing quick termination of unwanted or long-running responses. The implementation is robust, thread-safe, and maintains application state integrity while providing clear user feedback.