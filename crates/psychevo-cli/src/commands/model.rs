use std::env;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo::{
    Configuration, config::ConfigScope, config::ConfiguredModel, config::ModelCatalogEntry,
    config::ModelCatalogProvider,
};
use serde_json::{Value, json};

use crate::args::{
    ModelArgs, ModelCommand, ModelFetchArgs, ModelJsonArgs, ModelListArgs, ModelSetArgs,
};
use crate::commands::common::{CommandConfiguration, print_json_error};
use crate::env::{ensure_home_initialized, inherited_env, resolve_psychevo_home};

pub(crate) async fn run_model_command(args: ModelArgs) -> Result<ExitCode> {
    match run_model_command_inner(&args).await {
        Ok(code) => Ok(code),
        Err(err) if model_json(&args) => {
            print_json_error(&err)?;
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

pub(crate) async fn run_model_command_inner(args: &ModelArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    ensure_home_initialized(&home)?;
    let context = CommandConfiguration::open(&env_map, &home, &cwd).await?;
    let result = match &args.command {
        ModelCommand::List(args) => list_models(args, context.configuration()),
        ModelCommand::Current(args) => current_model(args, context.configuration()),
        ModelCommand::Set(args) => set_model(args, context.configuration()),
        ModelCommand::Fetch(args) => fetch_models(args, context.configuration()).await,
    };
    context.finish(result).await?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn list_models(args: &ModelListArgs, configuration: &Configuration) -> Result<()> {
    let mut models = configuration.configured_models()?;
    if let Some(provider) = &args.provider {
        let provider = provider.trim().to_lowercase();
        models.retain(|model| model.provider == provider);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "models": models.iter().map(model_value).collect::<Vec<_>>(),
            }))?
        );
    } else if models.is_empty() {
        println!("No configured models found.");
    } else {
        println!("Provider\tModel\tReasoning\tContext");
        for model in models {
            println!(
                "{}\t{}\t{}\t{}",
                model.provider,
                model.model,
                model.reasoning_effort.unwrap_or_else(|| "-".to_string()),
                model
                    .context_limit
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string())
            );
        }
    }
    Ok(())
}

pub(crate) fn current_model(args: &ModelJsonArgs, configuration: &Configuration) -> Result<()> {
    let selected = configuration.selected_model()?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "model": selected.as_ref().map(model_value),
            }))?
        );
    } else if let Some(model) = selected {
        println!("{}/{}", model.provider, model.model);
        println!("label: {}", model.provider_label);
        if let Some(reasoning) = model.reasoning_effort {
            println!("reasoning: {reasoning}");
        }
        if let Some(context) = model.context_limit {
            println!("context: {context}");
        }
    } else {
        println!("No model selected.");
    }
    Ok(())
}

pub(crate) fn set_model(args: &ModelSetArgs, configuration: &Configuration) -> Result<()> {
    let scope = if args.global {
        ConfigScope::Global
    } else {
        ConfigScope::Local
    };
    let value = configuration.set_default_model(scope, &args.model, None)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("model: {}", value["model"].as_str().unwrap_or("-"));
        println!("scope: {}", value["scope"].as_str().unwrap_or("-"));
        println!("path: {}", value["path"].as_str().unwrap_or("-"));
    }
    Ok(())
}

pub(crate) async fn fetch_models(
    args: &ModelFetchArgs,
    configuration: &Configuration,
) -> Result<()> {
    let mut providers = configuration.model_catalog_providers()?;
    if let Some(provider) = &args.provider {
        let provider = provider.trim().to_lowercase();
        providers.retain(|row| row.provider == provider);
        if providers.is_empty() {
            return Err(anyhow!("provider not found: {provider}"));
        }
    } else {
        providers.retain(ModelCatalogProvider::fetchable);
    }
    if providers.is_empty() {
        return Err(anyhow!("no fetchable model providers found"));
    }

    let mut rows = Vec::new();
    for provider in &providers {
        let models = configuration
            .fetch_and_cache_model_catalog(provider)
            .await?;
        rows.push(json!({
            "provider": provider_value(provider),
            "models": models.iter().map(catalog_model_value).collect::<Vec<_>>(),
        }));
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "providers": rows }))?
        );
    } else {
        for row in rows {
            let provider = row["provider"]["provider"].as_str().unwrap_or("-");
            println!("{provider}:");
            for model in row["models"].as_array().into_iter().flatten() {
                println!("  {}", model["id"].as_str().unwrap_or("-"));
            }
        }
    }
    Ok(())
}

pub(crate) fn model_value(model: &ConfiguredModel) -> Value {
    json!({
        "provider": model.provider,
        "provider_label": model.provider_label,
        "model": model.model,
        "reasoning_effort": model.reasoning_effort,
        "context_limit": model.context_limit,
        "metadata": model.metadata.public_json(),
    })
}

pub(crate) fn provider_value(provider: &ModelCatalogProvider) -> Value {
    json!({
        "provider": provider.provider,
        "label": provider.display_label,
        "base_url": provider.base_url,
        "api_key_env": provider.api_key_env,
        "missing_credentials": provider.missing_credentials,
        "unavailable_reason": provider.unavailable_reason,
        "no_auth": provider.no_auth,
    })
}

pub(crate) fn catalog_model_value(model: &ModelCatalogEntry) -> Value {
    json!({
        "id": model.id,
        "context_limit": model.context_limit,
        "metadata": model.metadata.public_json(),
    })
}

pub(crate) fn model_json(args: &ModelArgs) -> bool {
    match &args.command {
        ModelCommand::List(args) => args.json,
        ModelCommand::Current(args) => args.json,
        ModelCommand::Set(args) => args.json,
        ModelCommand::Fetch(args) => args.json,
    }
}
