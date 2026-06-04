use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use psychevo_runtime::{
    ScopedCustomProviderInput, create_scoped_custom_provider, set_default_model,
};

use crate::args::{DoctorArgs, InitArgs, SetupArgs};
use crate::commands::common::scoped_config_dir;
use crate::commands::doctor::run_doctor_command;
use crate::commands::init::run_init_command;
use crate::commands::serve::{
    resolve_static_dir_diagnostic, source_checkout_roots, static_install_share_dir,
};
use crate::env::{inherited_env, resolve_psychevo_home};

pub(crate) async fn run_setup_command(args: SetupArgs) -> Result<ExitCode> {
    if args.dry_run {
        print_dry_run();
        return Ok(ExitCode::SUCCESS);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("pevo setup is interactive and requires a terminal.");
        eprintln!("For non-interactive setup, use:");
        eprintln!("  pevo init");
        eprintln!(
            "  pevo auth setup --provider <id> --model <model> --base-url <url> --api-key-stdin"
        );
        eprintln!("  pevo doctor --json");
        return Ok(ExitCode::from(2));
    }

    println!("Psychevo setup");
    let _ = run_init_command(InitArgs { reset_state: false })?;

    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    configure_provider(&home, &cwd)?;
    maybe_install_web_assets(&env_map, &cwd)?;

    println!();
    println!("Doctor summary:");
    run_doctor_command(DoctorArgs {
        json: false,
        live: false,
    })
    .await
}

fn print_dry_run() {
    println!("pevo setup dry run");
    println!("steps:");
    println!("  pevo init");
    println!("  prompt for provider/model/base-url/auth");
    println!("  write scoped provider config and optional .env API key");
    println!("  check or install Workbench assets");
    println!("  pevo doctor");
}

fn configure_provider(home: &Path, cwd: &Path) -> Result<()> {
    println!();
    println!("Provider");
    let provider = prompt_default("provider id", "deepseek")?;
    let model = prompt_default("model id", "deepseek-chat")?;
    let base_url = prompt_default("base url", "https://api.deepseek.com/v1")?;
    let label = prompt_default("provider label", &provider)?;
    let use_auth = confirm_default("use API-key auth", true)?;
    let (api_key_env, api_key, no_auth) = if use_auth {
        let default_env = format!(
            "{}_API_KEY",
            provider.to_ascii_uppercase().replace('-', "_")
        );
        let env_name = prompt_default("API key env var", &default_env)?;
        let api_key = if confirm_default("paste API key now", false)? {
            Some(read_hidden("API key: ")?)
        } else {
            None
        };
        (Some(env_name), api_key, false)
    } else {
        (None, None, true)
    };

    let result = create_scoped_custom_provider(ScopedCustomProviderInput {
        config_dir: scoped_config_dir(home, cwd, true)?,
        provider_id: provider.clone(),
        label,
        base_url: base_url.clone(),
        api_key_env,
        api_key,
        require_api_key: false,
        no_auth,
    })?;
    let model_spec = format!("{}/{}", result.provider_id, model);
    let default_model = set_default_model(home, cwd, true, &model_spec)?;
    println!("provider: {}", result.provider_id);
    println!("model: {}", default_model["model"].as_str().unwrap_or("-"));
    println!("scope: global");
    if no_auth
        && !(base_url.starts_with("http://127.0.0.1")
            || base_url.starts_with("http://localhost")
            || base_url.starts_with("http://[::1]"))
    {
        eprintln!("warning: no-auth is enabled for a non-loopback URL");
    }
    Ok(())
}

fn maybe_install_web_assets(
    env_map: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<()> {
    println!();
    println!("Web UI assets");
    let assets = resolve_static_dir_diagnostic(None, env_map, cwd)?;
    if assets.found() {
        println!("found: {}", assets.path.display());
        return Ok(());
    }
    println!("missing: {}", assets.path.display());
    let Some(root) = source_checkout_roots(cwd).into_iter().next() else {
        println!("source checkout not found; run scripts/install.sh from a Psychevo checkout.");
        return Ok(());
    };
    if !command_exists("pnpm") {
        println!("pnpm not found; install pnpm or run scripts/install.sh --no-web.");
        return Ok(());
    }
    if !confirm_default("build and install Web UI assets now", true)? {
        return Ok(());
    }
    run_checked(
        Command::new("pnpm")
            .arg("install")
            .arg("--frozen-lockfile")
            .current_dir(&root),
    )?;
    run_checked(
        Command::new("pnpm")
            .arg("--filter")
            .arg("@psychevo/workbench")
            .arg("build")
            .current_dir(&root),
    )?;
    let dist = root.join("apps/workbench/dist");
    let Some(target) = static_install_share_dir() else {
        println!("could not resolve install-share directory for this pevo binary.");
        return Ok(());
    };
    copy_dir_replace(&dist, &target)?;
    println!("installed: {}", target.display());
    Ok(())
}

fn prompt_default(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let line = line.trim();
    Ok(if line.is_empty() {
        default.to_string()
    } else {
        line.to_string()
    })
}

fn confirm_default(label: &str, default: bool) -> Result<bool> {
    let marker = if default { "Y/n" } else { "y/N" };
    print!("{label} [{marker}]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let line = line.trim().to_ascii_lowercase();
    if line.is_empty() {
        return Ok(default);
    }
    Ok(matches!(line.as_str(), "y" | "yes"))
}

fn read_hidden(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    enable_raw_mode()?;
    let mut value = String::new();
    let result = loop {
        match event::read() {
            Ok(Event::Key(key))
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c') =>
            {
                break Err(anyhow!("secret input interrupted"));
            }
            Ok(Event::Key(key)) => match key.code {
                KeyCode::Enter => break Ok(value),
                KeyCode::Char(ch) => value.push(ch),
                KeyCode::Backspace => {
                    value.pop();
                }
                _ => {}
            },
            Ok(_) => {}
            Err(err) => break Err(err.into()),
        }
    };
    let _ = disable_raw_mode();
    eprintln!();
    result
}

fn command_exists(name: &str) -> bool {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .any(|dir| {
            let direct = dir.join(name);
            if direct.is_file() {
                return true;
            }
            cfg!(windows) && dir.join(format!("{name}.exe")).is_file()
        })
}

fn run_checked(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(anyhow!("command failed with status {status}"));
    }
    Ok(())
}

fn copy_dir_replace(source: &Path, target: &Path) -> Result<()> {
    if !source.join("index.html").exists() {
        return Err(anyhow!(
            "Workbench dist is missing index.html: {}",
            source.display()
        ));
    }
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::create_dir_all(target)?;
    copy_dir_contents(source, target)
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_dir_contents(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}
