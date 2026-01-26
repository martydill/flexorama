# Flexorama Hooks Support

Flexorama now supports Claude Code style hooks via `.flexorama` configuration. Hook definitions can live in either:

- `~/.flexorama` (global hooks)
- `<project>/.flexorama` (project hooks)

## Configuration formats

Flexorama loads hook definitions from the following files if present:

- `.flexorama/hooks.json`
- `.flexorama/hooks.yaml`
- `.flexorama/hooks.yml`
- `.flexorama/hooks.toml`
- `.flexorama/hooks` (extensionless JSON/YAML/TOML)

You can also place executable hook scripts inside `.flexorama/hooks/`. Each filename (without extension) becomes the event name.

Hook config files accept two formats:

```json
{
  "hooks": {
    "pre_message": [
      "echo pre-message",
      {"command": "./scripts/check.sh", "args": ["--fast"], "continue_on_error": true}
    ]
  }
}
```

or

```yaml
pre_message:
  - echo pre-message
  - command: ./scripts/check.sh
    args: [--fast]
    continue_on_error: true
```

## Supported events

Flexorama maps Claude-style hook events to the following names (aliases accepted):

- `pre_message`
- `post_message`
- `pre_tool`
- `post_tool`

## Hook execution behavior

- Hook commands receive a JSON payload on **stdin** describing the event.
- Hooks may return a JSON response to influence execution:
  - `{ "action": "abort", "message": "Reason" }` to cancel.
  - `{ "user_message": "Updated message" }` to replace the user message (pre-message only).
  - `{ "tool_arguments": { ... } }` to replace tool arguments (pre-tool only).
- Non-JSON stdout is ignored.
- Failures respect `continue_on_error` when set.

Environment variables set for hooks include:

- `CLAUDE_CODE_HOOK_EVENT`
- `CLAUDE_CODE_PROJECT_ROOT`
- `CLAUDE_CODE_HOOK_SOURCE` (`home` or `project`)

Flexorama also provides `FLEXORAMA_*` equivalents for the same values.
