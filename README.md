# AIxplosion

The hybrid cli/web coding agent that works your way.

<img width="898" height="571" alt="image" src="https://github.com/user-attachments/assets/324128e6-9bcd-4e78-bb5a-a0657ed23d64" />

## Features
 - Interactive and non-interactive mode
 - Built-in file editing, bash, code search, and glob tools
 - Claude-style skills support and management with /skills
 - Syntax highlighting for code snippets
 - Direct bash command execution with !
 - Adding context files with @path_to_file_name
 - <tab> autocomplete for file paths and commands
 - MCP support
 - Local and global AGENTS.md support
 - Bash command and file editing security model with easy adding of wildcard versions to your allow list and sensible defaults
 - Yolo mode for living dangerously
 - Customizable system prompt
 - Conversation history stored in a per-project Sqlite DB
 - Session resuming via /resume
 - Full text conversation search via /search
 - Plan mode and /plan command support for managing plans and toggling plan mode
 - Subagent support via /agent
 - Command line history navigation with up and down arrow keys and Ctrl-R search
 - Support for different LLM APIs (Anthropic, Gemini) with the --provider arg
 - Support for different models for each provider with /model


## Web interface

The optional web UI provides a ChatGPT-style browser-based interface for chats, as well as plan, agent, MCP, skills, and stats functionality

## Todo 
 - Git worktrees
 - Token speedometer
 - Hooks
 - Web search tool
 - Compacting
 - Pasting or referencing images
 - Custom slash commands
 - Memory editing
 - Sandboxing 


## Usage
### Provider: 
Specify a provider on the command line with --provider. 

Supported providers: 
 - openapi
 - gemini
 - z.ai
 - anthropic

### API token: 
Specify api token on the command line with --api-key, OR set an env var for your provider

Supported env vars: 
- OPENAI_API_KEY
- ZAI_API_KEY
- GEMINI_API_KEY
- ANTHROPIC_AUTH_TOKEN

### CLI version
```cargo run -- --stream --provider <provider>```

### Web version
```cargo run -- --web --provider <provider>```

### Example
``` cargo run -- --stream --provider openai --api-key ABCDasdfxxx...```


## License

This project is licensed under the MIT License.
