use std::str::FromStr;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::settings;
use crate::store::AppState;

#[derive(Debug, Clone, PartialEq)]
enum CliCommand {
    Switch { app: AppType, provider: String },
    List { app: AppType },
    Current { app: AppType },
    Help,
}

const CLI_USAGE: &str = r#"CC Switch CLI

Usage:
  cc-switch switch --app <app> --provider <provider-id-or-name>
  cc-switch list --app <app>
  cc-switch current --app <app>
  cc-switch help

Options:
  --app, -a             Target app: claude | codex | gemini | opencode | openclaw
  --provider, --id, -p  Provider id or name (for switch)
"#;

pub fn maybe_run_from_process_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    maybe_run_from_args(&args)
}

fn maybe_run_from_args(args: &[String]) -> Option<i32> {
    let command = match parse_cli_command(args) {
        Ok(Some(command)) => command,
        Ok(None) => return None,
        Err(err) => {
            eprintln!("{err}");
            eprintln!();
            eprintln!("{CLI_USAGE}");
            return Some(2);
        }
    };

    let exit_code = match execute_cli_command(command) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };

    Some(exit_code)
}

fn parse_cli_command(args: &[String]) -> Result<Option<CliCommand>, String> {
    if args.is_empty() {
        return Ok(None);
    }

    let sub = args[0].as_str();
    let rest = &args[1..];

    match sub {
        "switch" => Ok(Some(parse_switch_args(rest)?)),
        "list" => Ok(Some(parse_app_only_args(rest, "list")?)),
        "current" => Ok(Some(parse_app_only_args(rest, "current")?)),
        "help" | "--help" | "-h" => Ok(Some(CliCommand::Help)),
        _ => Ok(None),
    }
}

fn parse_switch_args(args: &[String]) -> Result<CliCommand, String> {
    let mut app: Option<AppType> = None;
    let mut provider: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--app" | "-a" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "Missing value for --app".to_string())?;
                app = Some(parse_app(value)?);
            }
            "--provider" | "--id" | "-p" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "Missing value for --provider".to_string())?;
                provider = Some(value.clone());
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => return Err(format!("Unknown option for switch: {other}")),
        }
        i += 1;
    }

    let app = app.ok_or_else(|| "Missing required option --app".to_string())?;
    let provider = provider.ok_or_else(|| "Missing required option --provider".to_string())?;

    Ok(CliCommand::Switch { app, provider })
}

fn parse_app_only_args(args: &[String], subcommand: &str) -> Result<CliCommand, String> {
    let mut app: Option<AppType> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--app" | "-a" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "Missing value for --app".to_string())?;
                app = Some(parse_app(value)?);
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => return Err(format!("Unknown option for {subcommand}: {other}")),
        }
        i += 1;
    }

    let app = app.ok_or_else(|| "Missing required option --app".to_string())?;

    Ok(match subcommand {
        "list" => CliCommand::List { app },
        "current" => CliCommand::Current { app },
        _ => return Err(format!("Unsupported subcommand: {subcommand}")),
    })
}

fn parse_app(value: &str) -> Result<AppType, String> {
    AppType::from_str(value).map_err(|e| e.to_string())
}

fn execute_cli_command(command: CliCommand) -> Result<(), String> {
    match command {
        CliCommand::Help => {
            println!("{CLI_USAGE}");
            Ok(())
        }
        CliCommand::List { app } => execute_list(app),
        CliCommand::Current { app } => execute_current(app),
        CliCommand::Switch { app, provider } => execute_switch(app, &provider),
    }
}

fn init_state() -> Result<AppState, String> {
    let db = Database::init().map_err(|e| format!("Failed to initialize database: {e}"))?;
    Ok(AppState::new(Arc::new(db)))
}

fn execute_list(app: AppType) -> Result<(), String> {
    let state = init_state()?;
    let providers = state
        .db
        .get_all_providers(app.as_str())
        .map_err(|e| format!("Failed to load providers: {e}"))?;

    if providers.is_empty() {
        println!("No providers configured for {}", app.as_str());
        return Ok(());
    }

    let current_id = settings::get_effective_current_provider(&state.db, &app)
        .map_err(|e| format!("Failed to resolve current provider: {e}"))?;

    for (id, provider) in &providers {
        let marker = if current_id.as_deref() == Some(id.as_str()) {
            "*"
        } else {
            " "
        };
        println!("{marker} {id}\t{}", provider.name);
    }

    Ok(())
}

fn execute_current(app: AppType) -> Result<(), String> {
    let state = init_state()?;
    let providers = state
        .db
        .get_all_providers(app.as_str())
        .map_err(|e| format!("Failed to load providers: {e}"))?;

    if providers.is_empty() {
        return Err(format!("No providers configured for {}", app.as_str()));
    }

    let current_id = settings::get_effective_current_provider(&state.db, &app)
        .map_err(|e| format!("Failed to resolve current provider: {e}"))?
        .ok_or_else(|| format!("No current provider set for {}", app.as_str()))?;

    if let Some(provider) = providers.get(&current_id) {
        println!("{}\t{}", provider.id, provider.name);
    } else {
        println!("{current_id}");
    }

    Ok(())
}

fn execute_switch(app: AppType, provider_ref: &str) -> Result<(), String> {
    let state = init_state()?;
    let providers = state
        .db
        .get_all_providers(app.as_str())
        .map_err(|e| format!("Failed to load providers: {e}"))?;

    if providers.is_empty() {
        return Err(format!("No providers configured for {}", app.as_str()));
    }

    let provider_id = resolve_provider_id(&providers, provider_ref, &app)?;
    let provider_name = providers
        .get(&provider_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| provider_id.clone());

    let result = ProviderService::switch(&state, app.clone(), &provider_id)
        .map_err(|e| format!("Switch failed: {e}"))?;

    println!(
        "Switched {} provider to {} ({})",
        app.as_str(),
        provider_name,
        provider_id
    );

    for warning in result.warnings {
        eprintln!("Warning: {warning}");
    }

    Ok(())
}

fn resolve_provider_id(
    providers: &IndexMap<String, Provider>,
    provider_ref: &str,
    app: &AppType,
) -> Result<String, String> {
    let provider_ref = provider_ref.trim();
    if provider_ref.is_empty() {
        return Err("Provider selector cannot be empty".to_string());
    }

    if providers.contains_key(provider_ref) {
        return Ok(provider_ref.to_string());
    }

    let target = provider_ref.to_lowercase();
    let exact_name_matches: Vec<String> = providers
        .iter()
        .filter_map(|(id, provider)| {
            if provider.name.to_lowercase() == target {
                Some(id.clone())
            } else {
                None
            }
        })
        .collect();

    if exact_name_matches.len() == 1 {
        return Ok(exact_name_matches[0].clone());
    }
    if exact_name_matches.len() > 1 {
        return Err(format!(
            "Provider name '{provider_ref}' is ambiguous: {}",
            format_candidates(providers, &exact_name_matches)
        ));
    }

    let fuzzy_matches: Vec<String> = providers
        .iter()
        .filter_map(|(id, provider)| {
            let id_match = id.to_lowercase().contains(&target);
            let name_match = provider.name.to_lowercase().contains(&target);
            if id_match || name_match {
                Some(id.clone())
            } else {
                None
            }
        })
        .collect();

    if fuzzy_matches.len() == 1 {
        return Ok(fuzzy_matches[0].clone());
    }
    if fuzzy_matches.len() > 1 {
        return Err(format!(
            "Provider selector '{provider_ref}' matched multiple providers: {}",
            format_candidates(providers, &fuzzy_matches)
        ));
    }

    Err(format!(
        "Provider '{provider_ref}' not found for {}. Run `cc-switch list --app {}` to inspect available providers.",
        app.as_str(),
        app.as_str()
    ))
}

fn format_candidates(providers: &IndexMap<String, Provider>, ids: &[String]) -> String {
    let mut values: Vec<String> = ids
        .iter()
        .map(|id| {
            let name = providers
                .get(id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            format!("{id} ({name})")
        })
        .collect();
    values.sort();
    values.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn sample_providers() -> IndexMap<String, Provider> {
        let mut map = IndexMap::new();
        map.insert(
            "p-openai".to_string(),
            Provider::with_id(
                "p-openai".to_string(),
                "OpenAI".to_string(),
                json!({}),
                None,
            ),
        );
        map.insert(
            "p-anthropic".to_string(),
            Provider::with_id(
                "p-anthropic".to_string(),
                "Anthropic".to_string(),
                json!({}),
                None,
            ),
        );
        map
    }

    #[test]
    fn parse_switch_command_success() {
        let parsed = parse_cli_command(&args(&[
            "switch",
            "--app",
            "claude",
            "--provider",
            "p-openai",
        ]))
        .expect("parse should succeed");

        assert_eq!(
            parsed,
            Some(CliCommand::Switch {
                app: AppType::Claude,
                provider: "p-openai".to_string()
            })
        );
    }

    #[test]
    fn parse_unknown_command_returns_none() {
        let parsed = parse_cli_command(&args(&["--register-protocol"])).expect("parse should work");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_switch_missing_provider_fails() {
        let err = parse_cli_command(&args(&["switch", "--app", "claude"]))
            .expect_err("missing provider should fail");
        assert!(err.contains("--provider"));
    }

    #[test]
    fn resolve_provider_by_id() {
        let providers = sample_providers();
        let resolved = resolve_provider_id(&providers, "p-openai", &AppType::Claude)
            .expect("id should resolve");
        assert_eq!(resolved, "p-openai");
    }

    #[test]
    fn resolve_provider_by_name_case_insensitive() {
        let providers = sample_providers();
        let resolved = resolve_provider_id(&providers, "openai", &AppType::Claude)
            .expect("name should resolve");
        assert_eq!(resolved, "p-openai");
    }

    #[test]
    fn resolve_provider_ambiguous_fails() {
        let mut providers = sample_providers();
        providers.insert(
            "p-openai-backup".to_string(),
            Provider::with_id(
                "p-openai-backup".to_string(),
                "OpenAI".to_string(),
                json!({}),
                None,
            ),
        );

        let err = resolve_provider_id(&providers, "OpenAI", &AppType::Claude)
            .expect_err("ambiguous name should fail");
        assert!(err.contains("ambiguous"));
    }

    #[test]
    fn parse_help_command() {
        let parsed = parse_cli_command(&args(&["help"])).expect("parse should succeed");
        assert_eq!(parsed, Some(CliCommand::Help));
    }
}
