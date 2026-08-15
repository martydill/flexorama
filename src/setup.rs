use anyhow::{anyhow, Result};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Select};

use crate::config::{Config, Provider};

/// Check if this is the first run (no config file exists)
pub fn is_first_run() -> bool {
    !Config::default_config_path().exists()
}

/// Display welcome banner
fn display_welcome() {
    println!();
    println!("{}", "═══════════════════════════════════════════════════".cyan());
    println!("{}", "  Welcome to Flexorama!".green().bold());
    println!("{}", "  AI-powered agent for code and task automation".white());
    println!("{}", "═══════════════════════════════════════════════════".cyan());
    println!();
    println!("Let's configure your AI provider.");
    println!();
}

/// Run the interactive setup wizard
pub async fn run_setup_wizard() -> Result<Config> {
    display_welcome();

    let theme = ColorfulTheme::default();

    // Provider selection
    println!("{}", "Select your AI provider:".white().bold());
    let provider = Select::with_theme(&theme)
        .items(&[
            "Anthropic (Claude)",
            "Google (Gemini)",
            "OpenAI (GPT)",
            "Mistral AI",
            "Z.ai (GLM)",
            "Ollama (local)",
        ])
        .default(0)
        .interact()?;

    let provider = match provider {
        0 => Provider::Anthropic,
        1 => Provider::Gemini,
        2 => Provider::OpenAI,
        3 => Provider::Mistral,
        4 => Provider::Zai,
        5 => Provider::Ollama,
        _ => return Err(anyhow!("Invalid provider selection")),
    };

    println!();
    println!("You selected: {}", provider.to_string().green().bold());

    let env_var_name = match provider {
        Provider::Anthropic => "ANTHROPIC_AUTH_TOKEN",
        Provider::Gemini => "GEMINI_API_KEY",
        Provider::OpenAI => "OPENAI_API_KEY",
        Provider::Mistral => "MISTRAL_API_KEY",
        Provider::Zai => "ZAI_API_KEY",
        Provider::Ollama => "OLLAMA_API_KEY (optional)",
    };

    println!();
    println!("{}", "API Key Setup:".white().bold());
    println!("Set your API key using one of these methods:");
    println!();
    println!("1. {} environment variable:", "Environment variable".cyan());
    println!("   export {}=your-api-key", env_var_name);
    println!();
    println!("2. {} on the command line:", "Command line flag".cyan());
    println!("   flexorama -k your-api-key -m \"your message\"");
    println!();
    println!("3. {} in your shell profile:", "Environment variable".cyan());
    println!("   Add to ~/.zshrc or ~/.bashrc:");
    println!("   echo 'export {}=your-api-key' >> ~/.zshrc", env_var_name);
    println!();

    if provider == Provider::Ollama {
        println!("{}", "Ollama runs locally and doesn't require an API key.".yellow());
        println!("Install Ollama from: {}", "https://ollama.com/download".cyan().underline());
        println!();
    }

    // Model selection
    println!("{}", "Select your default model:".white().bold());
    println!("(You can change this later with --model flag or in config)");

    let models = crate::config::provider_models(provider);
    let default_model = crate::config::provider_default_model(provider);
    let default_index = models
        .iter()
        .position(|m| m == &default_model)
        .unwrap_or(0);

    let model_selection = Select::with_theme(&theme)
        .items(models)
        .default(default_index)
        .interact()?;

    let selected_model = models[model_selection].to_string();

    println!();
    println!("{}", "Configuration Summary:".white().bold());
    println!("  Provider: {}", provider.to_string().green());
    println!("  Model: {}", selected_model.cyan());
    println!("  API Key: Set via {} environment variable or -k flag", env_var_name);
    println!();

    // Create and save config
    let config = Config {
        provider,
        base_url: crate::config::provider_default_base_url(provider),
        default_model: selected_model,
        api_key: String::new(), // API keys are not stored in config
        ..Config::default()
    };

    config.save(None).await?;
    println!("✓ Configuration saved to: {}", config.path().display().to_string().cyan());

    println!();
    println!("{}", "═══════════════════════════════════════════════════".cyan());
    println!("{}", "  Setup complete!".green().bold());
    println!("{}", "═══════════════════════════════════════════════════".cyan());
    println!();

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_first_run_detection() {
        // This test just verifies the function doesn't panic
        let _ = is_first_run();
    }
}
