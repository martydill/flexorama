# Plan Mode

Plan mode produces read-only implementation plans, blocks mutating tools, and saves the resulting plan to the SQLite database.

## Behavior
- Flag: `--plan-mode` (available on all invocation modes). When enabled the agent applies a plan-specific system prompt and announces activation.
- Tool policy: only read-only built-ins are exposed (`list_directory`, `read_file`, `search_in_files`). Mutating tools (`bash`, `write_file`, `edit_file`, `delete_file`, `create_directory`) and MCP tool refreshes are disabled; any attempted disallowed tool call returns an error.
- System prompt overlay: instructs the model to produce a Markdown plan only (goals, context/assumptions, ordered steps, risks, validation) and to avoid performing actions or requesting approvals.
- Plan persistence: once the final plan is produced, it is stored in the `plans` table and the response echoes `_Plan saved with ID: <id>_` when a database is configured.
- Slash commands:
  - `/plan on` enables plan mode mid-session (read-only tools, planning prompt applied).
  - `/plan off` disables plan mode and restores execution tools/system prompt.
  - `/plan run <id>` loads a saved plan (IDs may contain spaces) and executes it with plan mode off.

## Database
- New table `plans`: `id TEXT PRIMARY KEY`, `conversation_id TEXT NULL`, `title TEXT NULL`, `user_request TEXT NOT NULL`, `plan_markdown TEXT NOT NULL`, `created_at DATETIME DEFAULT CURRENT_TIMESTAMP`.
- Indexes: `idx_plans_conversation_id` and `idx_plans_created_at`.
- Persistence helper: `DatabaseManager::create_plan` and `ConversationManager::save_plan` handle inserts and conversation association.

## Usage
- Single message: `flexorama --plan-mode "Implement SSO with OAuth"` -> prints Markdown plan and saves it.
- Interactive: start with `flexorama --plan-mode`, every user message yields a new stored plan.

## Notes
- Context files still work; @file syntax continues to add read-only context.
- YOLO mode remains independent but is effectively moot in plan mode because mutating tools are not registered.
- Plan mode skips MCP tool refresh to avoid unknown side effects; MCP servers may still connect but their tools are withheld.
