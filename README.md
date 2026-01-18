# Flexorama

The hybrid cli/web coding agent that works your way.

<img width="1115" height="628" alt="image" src="https://github.com/user-attachments/assets/7be51786-f0d1-4724-99be-da3da15835c2" />
<img width="1290" height="719" alt="image" src="https://github.com/user-attachments/assets/fc96dd80-c8c6-43ce-b6a4-bb9bc8d27c97" />

## Features
 - Built-in file editing, bash, code search, and glob tools
 - Claude-style skills support and management with /skills
 - Claude-style custom slash command via ~/.flexorama/commands/
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
 - Support for different LLM APIs (Anthropic, Gemini, Mistral, OpenAI, Z.AI) with the --provider arg
 - Support for different models for each provider with /model
 - Local model support using the ollama provider with Ollama
 - Todo checklists 
 - Interactive and non-interactive mode


## Web interface

The optional web UI provides a ChatGPT-style browser-based interface for chats, as well as plan, agent, MCP, skills, and stats functionality

## Todo 
 - Git worktrees
 - Token speedometer
 - Hooks
 - Web search tool
 - Compacting
 - Pasting or referencing images
 - Memory editing
 - Sandboxing 


## Usage
### Provider: 
Specify a provider on the command line with --provider. 

Supported providers: 
 - openapi
 - gemini
 - mistral
 - z.ai
 - anthropic
 - ollama

### API token: 
Specify api token on the command line with --api-key, OR set an env var for your provider

Supported env vars: 
- OPENAI_API_KEY
- ZAI_API_KEY
- GEMINI_API_KEY
- MISTRAL_API_KEY
- ANTHROPIC_AUTH_TOKEN

### CLI version
```cargo run -- --provider <provider>```


### Web version
```cargo run -- --web --provider <provider>```


## License

This project is licensed under the MIT License.
