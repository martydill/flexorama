use colored::*;
use std::thread;

/// Print agent subcommand help information
pub fn print_agent_help() {
    app_println!("{}", "🤖 Subagent Commands".cyan().bold());
    app_println!();
    app_println!("{}", "Management:".green().bold());
    app_println!("  /agent list                    - List all subagents");
    app_println!("  /agent create <name> <prompt> - Create new subagent");
    app_println!("  /agent delete <name> [--confirm] - Delete subagent");
    app_println!("  /agent edit <name>             - Edit subagent configuration");
    app_println!("  /agent reload                  - Reload configurations from disk");
    app_println!();
    app_println!("{}", "Usage:".green().bold());
    app_println!("  /agent use <name>              - Switch to subagent");
    app_println!("  /agent switch <name>           - Alias for use");
    app_println!("  /agent exit                    - Exit subagent mode");
    app_println!("  /agent                         - Show current status");
    app_println!();
    app_println!("{}", "Examples:".green().bold());
    app_println!("  /agent create rust-expert \"You are a Rust expert...\"");
    app_println!("  /agent use rust-expert");
    app_println!("  /agent list");
    app_println!("  /agent exit");
    app_println!();
}

/// Print MCP help information
pub fn print_mcp_help() {
    app_println!("{}", "🔌 MCP Commands".cyan().bold());
    app_println!();
    app_println!("{}", "Server Management:".green().bold());
    app_println!("  /mcp list                    - List all MCP servers and their status");
    app_println!("  /mcp add <name> stdio <cmd>  - Add a stdio MCP server");
    app_println!("  /mcp add <name> ws <url>     - Add a WebSocket MCP server");
    app_println!("  /mcp add <name> http <url>   - Add an HTTP MCP server");
    app_println!("  /mcp remove <name>           - Remove an MCP server");
    app_println!("  /mcp connect <name>          - Connect to a specific server");
    app_println!("  /mcp disconnect <name>       - Disconnect from a specific server");
    app_println!("  /mcp reconnect <name>        - Reconnect to a specific server");
    app_println!("  /mcp connect-all             - Connect to all enabled servers");
    app_println!("  /mcp disconnect-all          - Disconnect from all servers");
    app_println!();
    app_println!("{}", "Testing & Debugging:".green().bold());
    app_println!("  /mcp test <command>          - Test if a command is available");
    app_println!("  /mcp tools                   - List all available MCP tools");
    app_println!();
    app_println!("{}", "Examples:".green().bold());
    app_println!("  /mcp test npx                - Test if npx is available");
    app_println!("  /mcp add myserver stdio npx -y @modelcontextprotocol/server-filesystem");
    app_println!("  /mcp add websocket ws://localhost:8080");
    app_println!("  /mcp add linear http https://mcp.linear.app/mcp");
    app_println!("  /mcp connect myserver");
    app_println!("  /mcp tools");
    app_println!();
}

/// Print file permissions help information
pub fn print_file_permissions_help() {
    app_println!("{}", "🔒 File Permissions Commands".cyan().bold());
    app_println!();
    app_println!("{}", "View File Permissions:".green().bold());
    app_println!(
        "  /file-permissions                - Show current file permissions and security settings"
    );
    app_println!("  /file-permissions show          - Alias for /file-permissions");
    app_println!("  /file-permissions list          - Alias for /file-permissions");
    app_println!("  /file-permissions help          - Show this help message");
    app_println!();
    app_println!("{}", "Testing:".green().bold());
    app_println!("  /file-permissions test <op> <path> - Test if file operation is allowed");
    app_println!("    Operations: write_file, edit_file, delete_file, create_directory");
    app_println!();
    app_println!("{}", "Security Settings:".green().bold());
    app_println!("  /file-permissions enable        - Enable file security");
    app_println!("  /file-permissions disable       - Disable file security");
    app_println!("  /file-permissions ask-on        - Enable asking for permission");
    app_println!("  /file-permissions ask-off       - Disable asking for permission");
    app_println!("  /file-permissions reset-session - Reset session permissions");
    app_println!();
    app_println!("{}", "Permission Options:".green().bold());
    app_println!("  When a file operation requires permission, you can choose:");
    app_println!("  • Allow this operation only - One-time permission");
    app_println!("  • Allow all file operations this session - Session-wide permission");
    app_println!("  • Deny this operation - Block the operation");
    app_println!();
    app_println!("{}", "Security Tips:".yellow().bold());
    app_println!("  • Enable 'ask for permission' for better security");
    app_println!("  • Use 'Allow this operation only' for one-off edits");
    app_println!("  • Use 'Allow all file operations this session' for trusted sessions");
    app_println!(
        "  • File operations include: write_file, edit_file, create_directory, delete_file"
    );
    app_println!("  • Read operations (read_file, list_directory) are always allowed");
    app_println!("  • Session permissions are reset when you restart the agent");
    app_println!();
    app_println!("{}", "Examples:".green().bold());
    app_println!("  /file-permissions test write_file /tmp/test.txt");
    app_println!("  /file-permissions enable");
    app_println!("  /file-permissions ask-on");
    app_println!("  /file-permissions reset-session");
    app_println!();
}

/// Print skill help information
pub fn print_skill_help() {
    app_println!("{}", "📚 Skill Commands".cyan().bold());
    app_println!();
    app_println!("{}", "Management:".green().bold());
    app_println!("  /skill list                    - List all skills and active status");
    app_println!("  /skill create <name> <desc>    - Create skill and SKILL.md");
    app_println!("  /skill update <path>           - Update skill from SKILL.md");
    app_println!("  /skill delete <name>           - Delete skill by name");
    app_println!("  /skill deactivate <name>       - Deactivate skill by name");
    app_println!("  /skill help                    - Show this help message");
    app_println!();
    app_println!("{}", "Examples:".green().bold());
    app_println!("  /skill list");
    app_println!("  /skill create rust \"Rust best practices\"");
    app_println!("  /skill update C:\\\\skills\\\\rust\\\\SKILL.md");
    app_println!("  /skill delete rust");
    app_println!("  /skill deactivate rust");
    app_println!();
}

/// Print permissions help information
pub fn print_permissions_help() {
    app_println!("{}", "🔒 Permissions Commands".cyan().bold());
    app_println!();
    app_println!("{}", "View Permissions:".green().bold());
    app_println!("  /permissions                - Show current permissions and security settings");
    app_println!("  /permissions show          - Alias for /permissions");
    app_println!("  /permissions list          - Alias for /permissions");
    app_println!("  /permissions help          - Show this help message");
    app_println!();
    app_println!("{}", "Manage Allowlist:".green().bold());
    app_println!("  /permissions allow <cmd>    - Add command to allowlist");
    app_println!("  /permissions remove-allow <cmd> - Remove from allowlist");
    app_println!();
    app_println!("{}", "Manage Denylist:".green().bold());
    app_println!("  /permissions deny <cmd>     - Add command to denylist");
    app_println!("  /permissions remove-deny <cmd> - Remove from denylist");
    app_println!();
    app_println!("{}", "Security Settings:".green().bold());
    app_println!("  /permissions enable         - Enable bash security");
    app_println!("  /permissions disable        - Disable bash security");
    app_println!("  /permissions ask-on         - Enable asking for permission");
    app_println!("  /permissions ask-off        - Disable asking for permission");
    app_println!();
    app_println!("{}", "Testing:".green().bold());
    app_println!("  /permissions test <cmd>     - Test if a command is allowed");
    app_println!();
    app_println!("{}", "Pattern Matching:".green().bold());
    app_println!("  • Use wildcards: 'git *' allows all git commands");
    app_println!("  • Use exact match: 'cargo test' allows only that command");
    app_println!("  • Prefix matching: 'git' matches 'git status', 'git log', etc.");
    app_println!();
    app_println!("{}", "Examples:".green().bold());
    app_println!("  /permissions allow 'git *'  - Allow all git commands");
    app_println!("  /permissions deny 'rm *'    - Deny dangerous rm commands");
    app_println!("  /permissions test 'ls -la'  - Test if ls -la is allowed");
    app_println!("  /permissions enable         - Turn security on");
    app_println!("  /permissions ask-on         - Ask for unknown commands");
    app_println!();
    app_println!("{}", "Security Tips:".yellow().bold());
    app_println!("  • Be specific with allowlist entries for better security");
    app_println!("  • Use denylist for dangerous command patterns");
    app_println!("  • Enable 'ask for permission' for unknown commands");
    app_println!("  • Changes are automatically saved to config file");
    app_println!();
}

/// Print the main help message
pub fn print_help() {
    app_println!("{}", "🤖 Flexorama - Slash Commands".cyan().bold());
    app_println!();
    app_println!("{}", "Available commands:".green().bold());
    app_println!("  /help         - Show this help message");
    app_println!("  /stats        - Show token usage statistics");
    app_println!("  /usage        - Show token usage statistics (alias for /stats)");
    app_println!("  /context      - Show current conversation context");
    app_println!("  /provider     - Show active LLM provider, model, and base URL");
    app_println!("  /model        - Show or set the active model");
    app_println!("  /search <q>   - Search previous conversations");
    app_println!("  /resume       - Resume a previous conversation");
    app_println!("  /clear        - Clear all conversation context (keeps AGENTS.md if it exists)");
    app_println!("  /reset-stats  - Reset token usage statistics");
    app_println!("  /permissions  - Manage bash command security permissions");
    app_println!("  /file-permissions  - Manage file operation security permissions");
    app_println!("  /mcp          - Manage MCP (Model Context Protocol) servers");
    app_println!("  /skill        - Manage skills (list, create, update, delete, deactivate)");
    app_println!("  /exit         - Exit the program");
    app_println!("  /quit         - Exit the program");
    app_println!();
    app_println!("{}", "Navigation:".green().bold());
    app_println!("  ↑ / ↓ Arrow   - Navigate through input history");
    app_println!("  ← / → Arrow   - Move cursor left/right in current input");
    app_println!("  Tab           - Auto-complete file paths and commands");
    app_println!("  Ctrl+R        - Start reverse history search (like readline)");
    app_println!("  ESC           - Cancel current AI conversation (during processing)");
    app_println!("  Ctrl+C        - Exit the program immediately");
    app_println!();
    app_println!("{}", "Reverse Search (Ctrl+R):".green().bold());
    app_println!("  Ctrl+R        - Start reverse search mode");
    app_println!("  Type text     - Search for matching history entries");
    app_println!("  Ctrl+R / r    - Find next match");
    app_println!("  ↑ / ↓ Arrow   - Navigate between matches");
    app_println!("  Enter         - Accept current match");
    app_println!("  ESC           - Cancel search and restore original input");
    app_println!("  Backspace     - Remove last character from search query");
    app_println!();
    app_println!("{}", "Shell Commands:".green().bold());
    app_println!("  !<command>    - Execute a shell command directly (bypasses all security)");
    app_println!("  Examples: !dir, !ls -la, !git status, !cargo test");
    app_println!("  Note: ! commands execute immediately without permission checks");
    app_println!();
    app_println!("{}", "Security Commands:".green().bold());
    app_println!("  /permissions              - Show current bash security settings");
    app_println!("  /file-permissions        - Show current file security settings");
    app_println!("  /permissions allow <cmd>  - Add command to allowlist");
    app_println!("  /permissions deny <cmd>   - Add command to denylist");
    app_println!("  /permissions test <cmd>  - Test if command is allowed");
    app_println!("  /file-permissions test <op> <path> - Test if file operation is allowed");
    app_println!("  /plan on|off             - Toggle plan mode at runtime");
    app_println!("  /plan run <id>           - Load and execute a saved plan by ID");
    app_println!();
    app_println!("{}", "MCP Commands:".green().bold());
    app_println!("  /mcp list                    - List MCP servers");
    app_println!("  /mcp add <name> stdio <cmd>  - Add stdio server");
    app_println!("  /mcp add <name> ws <url>     - Add WebSocket server");
    app_println!("  /mcp add <name> http <url>   - Add HTTP server");
    app_println!("  /mcp test <command>          - Test command availability");
    app_println!("  /mcp connect <name>          - Connect to server");
    app_println!("  /mcp tools                   - List available tools");
    app_println!("  /mcp help                    - Show MCP help");
    app_println!();
    app_println!("{}", "Context Files:".green().bold());
    app_println!("  Use -f or --file to include files as context");
    app_println!("  Use @path/to/file syntax in messages to auto-include files");
    app_println!("  AGENTS.md is automatically included from ~/.flexorama/AGENTS.md (priority)");
    app_println!("  Falls back to ./AGENTS.md if home directory version doesn't exist");
    app_println!("  Messages with only @file references will NOT make API calls");
    app_println!();
    app_println!("{}", "System Prompts:".green().bold());
    app_println!("  Use -s or --system to set a custom system prompt");
    app_println!("  System prompts set the behavior and personality of the AI");
    app_println!();
    app_println!("{}", "Streaming:".green().bold());
    app_println!("  Streaming is the default for all CLI modes");
    app_println!("  Use --no-stream to disable streaming (spinner + formatted response)");
    app_println!();
    app_println!("{}", "Plan Mode:".green().bold());
    app_println!("  Use --plan-mode to generate a read-only plan in Markdown");
    app_println!("  Plan mode disables mutating tools and saves the plan to the database");
    app_println!();
    app_println!("{}", "Examples:".green().bold());
    app_println!("  flexorama -f config.toml \"Explain this configuration\"");
    app_println!("  flexorama \"What does @Cargo.toml contain?\"");
    app_println!("  flexorama \"Compare @file1.rs and @file2.rs\"");
    app_println!("  flexorama \"@file1.txt @file2.txt\"  # Only adds context, no API call");
    app_println!("  flexorama -s \"You are a Rust expert\" \"Help me with this code\"");
    app_println!("  flexorama -s \"Act as a code reviewer\" -f main.rs \"Review this code\"");
    app_println!("  flexorama \"Tell me a story\"  # Streaming by default");
    app_println!("  flexorama --no-stream \"Tell me a story\"  # Disable streaming");
    app_println!("  flexorama --plan-mode \"Add Stripe billing\"  # Plan only, saves to DB");
    app_println!("  !dir                    # List directory contents");
    app_println!("  !git status             # Check git status");
    app_println!("  !cargo build            # Build the project");
    app_println!("  ESC                     # Cancel AI conversation during processing");
    app_println!();
    app_println!("{}", "History Navigation:".green().bold());
    app_println!("  • Press UP arrow to cycle through previous commands");
    app_println!("  • Press DOWN arrow to cycle through more recent commands");
    app_println!("  • Press Ctrl+R to start reverse history search");
    app_println!("  • Start typing to exit history navigation mode");
    app_println!("  • History is preserved across the entire session");
    app_println!("  • Duplicate and empty commands are not stored");
    app_println!();
    app_println!(
        "{}",
        "Any other input will be sent to the Flexorama for processing.".dimmed()
    );
    app_println!();
}

/// Display a large red warning for yolo mode
pub fn display_yolo_warning() {
    app_println!();
    app_println!(
        "{}",
        "⚠️  WARNING: YOLO MODE ENABLED  ⚠️".red().bold().blink()
    );
    app_println!(
        "{}",
        " ALL SECURITY PERMISSIONS ARE BYPASSED - USE WITH EXTREME CAUTION "
            .red()
            .bold()
    );
    app_println!();
    app_println!(
        "{}",
        " • File operations (read/write/delete) will execute WITHOUT prompts ".red()
    );
    app_println!(
        "{}",
        " • Bash commands will execute WITHOUT permission checks ".red()
    );
    app_println!(
        "{}",
        " • MCP tools will execute WITHOUT security validation ".red()
    );
    app_println!(
        "{}",
        " • No allowlist/denylist filtering will be applied ".red()
    );
    app_println!("{}", " • All tool calls are automatically approved ".red());
    app_println!();
    app_println!(
        "{}",
        " 🚨 This mode can cause irreversible damage to your system! "
            .red()
            .bold()
    );
    app_println!();
    app_println!(
        "{}",
        " Press Ctrl+C NOW to cancel if this was not intended! "
            .red()
            .bold()
    );
    app_println!();

    // Add a dramatic pause for effect
    thread::sleep(std::time::Duration::from_millis(2000));
    app_println!(
        "{}",
        "🔥 Proceeding in YOLO mode... You have been warned! 🔥"
            .red()
            .bold()
    );
    app_println!();
}

/// Display YOLO mode warning after MCP configuration is complete
pub fn display_mcp_yolo_warning() {
    app_println!();
    app_println!(
        "{}",
        "🔌 MCP Configuration Complete - YOLO Mode Active 🔌"
            .red()
            .bold()
    );
    app_println!();
    app_println!(
        "{}",
        " ⚠️  MCP TOOLS WILL EXECUTE WITHOUT SECURITY VALIDATION ⚠️ "
            .red()
            .bold()
    );
    app_println!();
    app_println!(
        "{}",
        " • MCP server tools are now available and will execute WITHOUT prompts ".red()
    );
    app_println!(
        "{}",
        " • No permission checks will be applied to MCP tool calls ".red()
    );
    app_println!(
        "{}",
        " • All MCP operations (file access, commands, etc.) are auto-approved ".red()
    );
    app_println!(
        "{}",
        " • External MCP server connections have unrestricted access ".red()
    );
    app_println!();
    app_println!(
        "{}",
        " 🚨 MCP tools can potentially access and modify your system! "
            .red()
            .bold()
    );
    app_println!();
    app_println!(
        "{}",
        " 🔥 All MCP servers and their tools are operating in YOLO mode! 🔥"
            .red()
            .bold()
    );
    app_println!();
}
