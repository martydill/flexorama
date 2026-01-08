# Gemini Provider Support

Flexorama can now use Google's Gemini API in addition to Anthropic. This guide covers how to switch providers and configure credentials.

## Configuration
- Set the provider to `gemini` via `--provider gemini` or in your config file.
- API key env vars (preferred): `GEMINI_API_KEY` (or `GOOGLE_API_KEY` fallback).
- Default base URL: `https://generativelanguage.googleapis.com/v1beta`. Override with `GEMINI_BASE_URL` if needed.
- Default model when using Gemini: `gemini-flash-latest`. Override with `--model` or the config file.

Example CLI usage:
```
flexorama --provider gemini --model gemini-1.5-pro -m "hello"
```

Example config excerpt (`~/.config/flexorama/config.toml`):
```toml
provider = "gemini"
base_url = "https://generativelanguage.googleapis.com/v1beta"
default_model = "gemini-flash-latest"
```

## Behavior Notes
- Tool/function calls are mapped to Gemini function declarations automatically.
- Streaming requests fall back to sending the full Gemini response as a single streamed chunk for now.
- Anthropic remains the default provider; switching providers updates defaults (base URL, model, and env-var lookup for API keys).
