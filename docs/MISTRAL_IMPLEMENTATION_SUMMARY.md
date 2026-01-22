Add Mistral as a new provider

I have added Mistral as a new LLM provider to the project. This involved the following changes:

1.  **Created `src/mistral.rs`:** Implemented `MistralClient` which handles API communication with Mistral's servers. It supports both standard message creation and streaming, largely mirroring the OpenAI implementation due to API similarities.
2.  **Updated `src/config.rs`:**
    *   Added `Mistral` to the `Provider` enum.
    *   Added default configuration for Mistral (API key env var `MISTRAL_API_KEY`, base URL `https://api.mistral.ai/v1`).
    *   Added a list of Mistral models (e.g., `mistral-large-latest`, `mistral-medium-latest`).
3.  **Updated `src/llm.rs`:**
    *   Integrated `MistralClient` into the `LlmClient` struct.
    *   Updated the `dispatch_to_provider!` macro to handle the `Mistral` provider.
    *   Added tests to verify the integration.
4.  **Updated `src/lib.rs`:** Exported the `mistral` module.
5.  **Updated `src/main.rs`:** Added `Provider::Mistral` to the API key validation logic to fix a compilation error.
6.  **Updated Frontend (`web/app.js`):**
    *   Updated `extractProvider` to correctly recognize "Mistral" in model names so it is displayed correctly in the UI charts.
    *   Updated chart creation functions (`createProvidersChart`, `createConversationsByProviderChart`, etc.) to use dynamic colors instead of hardcoded lists, ensuring Mistral and other new providers are correctly represented.
7.  **Updated Backend (`src/web.rs`):** Updated `extract_provider_from_model` to correctly identify Mistral models for API-based stats aggregation.
8.  **Updated Documentation (`README.md`):** Added Mistral to the list of supported providers. Updated the features list in `README.md` to include Mistral. (Note: Changes to `AGENTS.md` were reverted as per user request).

The changes have been verified with unit tests in `src/llm.rs` and by running `cargo check`.
