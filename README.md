# Flexorama

The hybrid cli/web coding agent that works your way.

<img width="1115" height="618" alt="image" src="https://github.com/user-attachments/assets/c39ad561-2021-4d83-88ae-12842338b9fb" />
<img width="1217" height="786" alt="image" src="https://github.com/user-attachments/assets/a6aca727-c29b-4275-980c-d6bffd37aad1" />

## Features
 - Built-in file editing, bash, code search, and glob tools
 - Claude-style skills support and management with /skills
 - Claude-style custom slash command via ~/.flexorama/commands/
 - Syntax highlighting for code snippets
 - Direct bash command execution with !
 - Adding context files with @path_to_file_name
 - Image support (for models that support it)
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
 - Support for different LLM APIs (Anthropic, Gemini, Mistral, OpenAI, Z.AI) with the --provider arg
 - Support for different models for each provider with /model
 - Local model support using the ollama provider with Ollama
 - Todo checklists 
 - Interactive and non-interactive mode
 - [Agent Client Protocol (ACP)](https://agentclientprotocol.com/overview/introduction) support for editor integration
 - Limited Claude Code-style [hook](https://code.claude.com/docs/en/hook) support (UserPromptSubmit, PreToolUse, PostToolUse, Stop, SubagentStop, SessionStart, PermissionRequest, with some restrictions)


## Web interface

The optional web UI provides a ChatGPT-style browser-based interface for chats, as well as plan, agent, MCP, skills, and stats functionality

## Todo 
 - Git worktrees
 - Token speedometer
 - Hooks
 - Web search tool
 - Compacting
 - Memory editing
 - Sandboxing 


## First Run Setup

On first run, Flexorama launches an interactive setup wizard that guides you through:

1. **Provider Selection** - Choose from Anthropic (Claude), Google (Gemini), OpenAI (GPT), Mistral AI, Z.ai (GLM), or Ollama (local)
2. **API Key Instructions** - Shows how to set your API key via environment variables or command line
3. **Model Selection** - Choose your default model from provider options

To re-run setup at any time:
```bash
flexorama --setup
```

## Usage
### Provider:
Specify a provider on the command line with --provider, or select during setup.

Supported providers:
 - openai
 - gemini
 - mistral
 - z.ai
 - anthropic
 - ollama

### API token:
Set API key via environment variable or command line:

Supported env vars:
- OPENAI_API_KEY
- ZAI_API_KEY
- GEMINI_API_KEY (or GOOGLE_API_KEY)
- MISTRAL_API_KEY
- ANTHROPIC_AUTH_TOKEN

Or use command line flag:
```bash
flexorama -k your-api-key -m "your message"
```

### CLI version
```cargo run -- --provider <provider>```


### Web version
```cargo run -- --web --provider <provider>```


## License

This project is licensed under the MIT License.
