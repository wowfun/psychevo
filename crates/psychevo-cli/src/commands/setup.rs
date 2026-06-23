use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use psychevo_runtime::{
    ModelCatalogEntry, fetch_model_catalog, model_catalog_providers, set_default_model,
    set_provider_api_key,
};

use crate::args::{DoctorArgs, InitArgs, SetupArgs};
use crate::commands::common::{base_run_options, scoped_config_dir};
use crate::commands::doctor::run_doctor_command;
use crate::commands::init::run_init_command;
use crate::commands::serve::{
    resolve_static_dir_diagnostic, source_checkout_roots, static_install_share_dir,
};
use crate::env::{inherited_env, resolve_psychevo_home};
use crate::provider_setup::{
    ProviderSetupBaseUrl, default_provider_setup_api_key_env, looks_like_api_key,
    provider_setup_presets, upsert_provider_options, validate_api_key_env, validate_base_url,
    validate_custom_setup_provider_id,
};

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
    configure_provider(&home, &cwd, &env_map).await?;
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

async fn configure_provider(
    home: &Path,
    cwd: &Path,
    env_map: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let mut io = TerminalSetupIo;
    configure_provider_with_io(home, cwd, env_map, &mut io).await
}

#[derive(Debug, Clone)]
struct SetupProviderSelection {
    provider_id: String,
    label: String,
    default_model: String,
    base_urls: Vec<ProviderSetupBaseUrl>,
    api_key_env_candidates: Vec<String>,
}

trait SetupIo {
    fn print_line(&mut self, line: &str) -> Result<()>;
    fn prompt_line(&mut self, prompt: &str) -> Result<String>;
    fn prompt_secret(&mut self, prompt: &str) -> Result<String>;
}

struct TerminalSetupIo;

impl SetupIo for TerminalSetupIo {
    fn print_line(&mut self, line: &str) -> Result<()> {
        println!("{line}");
        Ok(())
    }

    fn prompt_line(&mut self, prompt: &str) -> Result<String> {
        print!("{prompt}");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        Ok(line.trim().to_string())
    }

    fn prompt_secret(&mut self, prompt: &str) -> Result<String> {
        read_hidden(prompt)
    }
}

async fn configure_provider_with_io<I: SetupIo>(
    home: &Path,
    cwd: &Path,
    env_map: &std::collections::BTreeMap<String, String>,
    io: &mut I,
) -> Result<()> {
    io.print_line("")?;
    io.print_line("Provider")?;
    let provider = choose_provider(io)?;
    let base_url = choose_base_url(io, &provider)?;
    let default_api_key_env = default_api_key_env(&provider, env_map);
    let api_key_env = prompt_api_key_env(io, &default_api_key_env)?;
    let secret_prompt = if env_map
        .get(&api_key_env)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        format!("API key [found in {api_key_env}; Enter to reuse]: ")
    } else {
        "API key [Enter to set later]: ".to_string()
    };
    let api_key = io.prompt_secret(&secret_prompt)?;

    let config_dir = scoped_config_dir(home, cwd, true)?;
    upsert_provider_options(
        &config_dir,
        &provider.provider_id,
        &provider.label,
        &base_url,
        &api_key_env,
    )?;
    if !api_key.trim().is_empty() {
        let options = base_run_options(env_map, home, cwd)?;
        set_provider_api_key(&options, config_dir, &provider.provider_id, &api_key)?;
    }

    let model = select_model(home, cwd, env_map, &provider, io).await?;
    let model_spec = format!("{}/{}", provider.provider_id, model);
    let default_model = set_default_model(home, cwd, true, &model_spec)?;
    io.print_line(&format!("provider: {}", provider.provider_id))?;
    io.print_line(&format!(
        "model: {}",
        default_model["model"].as_str().unwrap_or("-")
    ))?;
    io.print_line("scope: global")?;
    Ok(())
}

fn choose_provider<I: SetupIo>(io: &mut I) -> Result<SetupProviderSelection> {
    let rows = provider_setup_presets();
    for (index, preset) in rows.iter().enumerate() {
        let id = preset.provider_id.unwrap_or("custom");
        io.print_line(&format!("  {}. {} ({})", index + 1, preset.label, id))?;
    }
    let choice = prompt_index(io, "provider", 1, rows.len())?;
    let preset = rows[choice - 1];
    if let Some(provider_id) = preset.provider_id {
        return Ok(SetupProviderSelection {
            provider_id: provider_id.to_string(),
            label: preset.label.to_string(),
            default_model: preset.default_model.to_string(),
            base_urls: preset.base_urls.to_vec(),
            api_key_env_candidates: preset
                .api_key_env_candidates
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        });
    }

    let provider_id = loop {
        let value = prompt_default_io(io, "provider id", "custom-provider")?;
        match validate_custom_setup_provider_id(&value) {
            Ok(()) => break value,
            Err(err) => io.print_line(&format!("invalid provider id: {err}"))?,
        }
    };
    Ok(SetupProviderSelection {
        label: provider_id.clone(),
        default_model: String::new(),
        base_urls: preset.base_urls.to_vec(),
        api_key_env_candidates: vec![default_provider_setup_api_key_env(
            preset.api_key_env_candidates,
            &std::collections::BTreeMap::new(),
            &provider_id,
        )],
        provider_id,
    })
}

fn choose_base_url<I: SetupIo>(io: &mut I, provider: &SetupProviderSelection) -> Result<String> {
    io.print_line("")?;
    io.print_line("Base URL")?;
    let value = if provider.base_urls.len() == 1 {
        prompt_default_io(io, "base url", provider.base_urls[0].url)?
    } else {
        for (index, choice) in provider.base_urls.iter().enumerate() {
            io.print_line(&format!(
                "  {}. {} ({})",
                index + 1,
                choice.label,
                choice.url
            ))?;
        }
        let custom_index = provider.base_urls.len() + 1;
        io.print_line(&format!("  {custom_index}. Custom"))?;
        let choice = prompt_index(io, "base URL choice", 1, custom_index)?;
        if choice == custom_index {
            prompt_default_io(io, "base url", provider.base_urls[0].url)?
        } else {
            provider.base_urls[choice - 1].url.to_string()
        }
    };
    validate_base_url(&value)
}

fn prompt_api_key_env<I: SetupIo>(io: &mut I, default: &str) -> Result<String> {
    loop {
        io.print_line("")?;
        io.print_line("API key env var")?;
        io.print_line(&format!("  Using {default}."))?;
        io.print_line("  The API key itself is entered next and will be hidden.")?;
        if !confirm_default_io(io, "change env var name", false)? {
            return Ok(default.to_string());
        }

        loop {
            let value = prompt_default_io(io, "env var name", default)?;
            if looks_like_api_key(&value) {
                io.print_line(
                    "That looks like an API key; enter the key at the hidden prompt next.",
                )?;
                break;
            }
            match validate_api_key_env(&value) {
                Ok(value) => return Ok(value),
                Err(_) => {
                    io.print_line("env var name must be a valid environment variable name")?
                }
            }
        }
    }
}

async fn select_model<I: SetupIo>(
    home: &Path,
    cwd: &Path,
    env_map: &std::collections::BTreeMap<String, String>,
    provider: &SetupProviderSelection,
    io: &mut I,
) -> Result<String> {
    io.print_line("")?;
    io.print_line("Model")?;
    match fetch_models_for_setup(home, cwd, env_map, &provider.provider_id).await {
        Ok(models) if !models.is_empty() => {
            io.print_line("Fetched models:")?;
            for (index, model) in models.iter().enumerate() {
                io.print_line(&format!("  {}. {}", index + 1, model.id))?;
            }
            let custom_index = models.len() + 1;
            io.print_line(&format!("  {custom_index}. Custom model id"))?;
            let choice = prompt_index(io, "model", 1, custom_index)?;
            if choice == custom_index {
                prompt_model_id(io, &provider.default_model)
            } else {
                Ok(models[choice - 1].id.clone())
            }
        }
        Ok(_) => {
            io.print_line("No models returned; enter a model id.")?;
            prompt_model_id(io, &provider.default_model)
        }
        Err(err) => {
            io.print_line(&format!(
                "Could not fetch models: {}",
                truncate_setup_error(&err.to_string())
            ))?;
            prompt_model_id(io, &provider.default_model)
        }
    }
}

async fn fetch_models_for_setup(
    home: &Path,
    cwd: &Path,
    env_map: &std::collections::BTreeMap<String, String>,
    provider_id: &str,
) -> Result<Vec<ModelCatalogEntry>> {
    let options = base_run_options(env_map, home, cwd)?;
    let providers = model_catalog_providers(&options)?;
    let provider = providers
        .into_iter()
        .find(|provider| provider.provider == provider_id)
        .ok_or_else(|| anyhow!("provider not found: {provider_id}"))?;
    if !provider.fetchable() {
        let reason = provider
            .missing_credentials
            .or(provider.unavailable_reason)
            .unwrap_or_else(|| "provider is not fetchable".to_string());
        return Err(anyhow!("{reason}"));
    }
    Ok(fetch_model_catalog(&provider).await?)
}

fn prompt_model_id<I: SetupIo>(io: &mut I, default_model: &str) -> Result<String> {
    if default_model.trim().is_empty() {
        prompt_required_io(io, "model id")
    } else {
        prompt_default_io(io, "model id", default_model)
    }
}

fn default_api_key_env(
    provider: &SetupProviderSelection,
    env_map: &std::collections::BTreeMap<String, String>,
) -> String {
    let candidates = provider
        .api_key_env_candidates
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    default_provider_setup_api_key_env(&candidates, env_map, &provider.provider_id)
}

fn prompt_index<I: SetupIo>(io: &mut I, label: &str, default: usize, max: usize) -> Result<usize> {
    loop {
        let value = prompt_default_io(io, label, &default.to_string())?;
        match value.parse::<usize>() {
            Ok(index) if (1..=max).contains(&index) => return Ok(index),
            _ => io.print_line(&format!("enter a number from 1 to {max}"))?,
        }
    }
}

fn prompt_default_io<I: SetupIo>(io: &mut I, label: &str, default: &str) -> Result<String> {
    let line = io.prompt_line(&format!("{label} [{default}]: "))?;
    Ok(if line.trim().is_empty() {
        default.to_string()
    } else {
        line.trim().to_string()
    })
}

fn confirm_default_io<I: SetupIo>(io: &mut I, label: &str, default: bool) -> Result<bool> {
    let marker = if default { "Y/n" } else { "y/N" };
    loop {
        let line = io.prompt_line(&format!("{label} [{marker}]: "))?;
        let line = line.trim().to_ascii_lowercase();
        if line.is_empty() {
            return Ok(default);
        }
        match line.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => io.print_line("enter y or n")?,
        }
    }
}

fn prompt_required_io<I: SetupIo>(io: &mut I, label: &str) -> Result<String> {
    loop {
        let line = io.prompt_line(&format!("{label}: "))?;
        if !line.trim().is_empty() {
            return Ok(line.trim().to_string());
        }
        io.print_line(&format!("{label} is required"))?;
    }
}

fn truncate_setup_error(value: &str) -> String {
    let trimmed = value.trim().replace(['\r', '\n', '\t'], " ");
    if trimmed.chars().count() <= 160 {
        trimmed
    } else {
        let mut out = trimmed.chars().take(157).collect::<String>();
        out.push_str("...");
        out
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{BTreeMap, VecDeque};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use tempfile::{TempDir, tempdir};

    struct TestIo {
        lines: VecDeque<String>,
        secrets: VecDeque<String>,
        output: String,
    }

    impl TestIo {
        fn new(lines: Vec<String>, secrets: Vec<String>) -> Self {
            Self {
                lines: VecDeque::from(lines),
                secrets: VecDeque::from(secrets),
                output: String::new(),
            }
        }
    }

    impl SetupIo for TestIo {
        fn print_line(&mut self, line: &str) -> Result<()> {
            self.output.push_str(line);
            self.output.push('\n');
            Ok(())
        }

        fn prompt_line(&mut self, prompt: &str) -> Result<String> {
            self.output.push_str(prompt);
            self.lines
                .pop_front()
                .ok_or_else(|| anyhow!("missing test input for {prompt}"))
        }

        fn prompt_secret(&mut self, prompt: &str) -> Result<String> {
            self.output.push_str(prompt);
            self.secrets
                .pop_front()
                .ok_or_else(|| anyhow!("missing test secret for {prompt}"))
        }
    }

    struct SetupCatalogServer {
        base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl SetupCatalogServer {
        fn new(body: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("addr");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let requests_for_thread = Arc::clone(&requests);
            thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = read_http_request(&mut stream);
                    requests_for_thread.lock().expect("requests").push(request);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
            });
            Self {
                base_url: format!("http://{addr}/v1"),
                requests,
            }
        }
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).expect("request");
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
            if data.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8_lossy(&data).to_string()
    }

    fn setup_workspace() -> (TempDir, PathBuf, PathBuf, BTreeMap<String, String>) {
        let temp = tempdir().expect("temp");
        let home = temp.path().join("psychevo-home");
        let workdir = temp.path().join("work");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::write(
            home.join("config.toml"),
            crate::commands::init::STARTER_CONFIG,
        )
        .expect("config");
        fs::write(home.join(".env"), "").expect("env");
        let env_map = BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().to_string_lossy().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                home.to_string_lossy().to_string(),
            ),
        ]);
        (temp, home, workdir, env_map)
    }

    fn unused_loopback_base_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        format!("http://{addr}/v1")
    }

    #[test]
    fn api_key_env_paste_detection_handles_secret_shapes() {
        assert!(looks_like_api_key("sk-pasted-secret-value"));
        assert!(looks_like_api_key("abc123def456ghi789jkl012mno345pqr678"));
        assert!(!looks_like_api_key("CUSTOM_API_KEY"));
        assert!(!looks_like_api_key("XIAOMI_TOKEN_PLAN_API_KEY"));
    }

    #[tokio::test]
    async fn deepseek_setup_fetches_models_and_hides_secret() {
        let (_temp, home, workdir, env_map) = setup_workspace();
        let server = SetupCatalogServer::new(r#"{"data":[{"id":"remote-model"}]}"#);
        let mut io = TestIo::new(
            vec![
                "1".to_string(),
                server.base_url.clone(),
                String::new(),
                "1".to_string(),
            ],
            vec!["secret-key".to_string()],
        );

        configure_provider_with_io(&home, &workdir, &env_map, &mut io)
            .await
            .expect("setup");

        let config = fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(config.contains(&format!("base_url = \"{}\"", server.base_url)));
        assert!(config.contains("api_key_env = \"DEEPSEEK_API_KEY\""));
        assert!(config.contains("model = \"deepseek/remote-model\""));
        let env = fs::read_to_string(home.join(".env")).expect("env");
        assert_eq!(env, "DEEPSEEK_API_KEY=secret-key\n");
        assert!(
            io.output
                .contains("API key env var\n  Using DEEPSEEK_API_KEY.")
        );
        assert!(!io.output.contains("API key env var [DEEPSEEK_API_KEY]:"));
        assert!(!io.output.contains("secret-key"));
        let requests = server.requests.lock().expect("requests");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /v1/models HTTP/1.1"));
        assert!(
            requests[0]
                .to_lowercase()
                .contains("authorization: bearer secret-key")
        );
    }

    #[tokio::test]
    async fn zai_setup_defaults_to_general_and_allows_coding_plan() {
        for (choice, expected) in [
            ("", "https://api.z.ai/api/paas/v4"),
            ("2", "https://api.z.ai/api/coding/paas/v4"),
        ] {
            let (_temp, home, workdir, env_map) = setup_workspace();
            let mut io = TestIo::new(
                vec![
                    "2".to_string(),
                    choice.to_string(),
                    String::new(),
                    String::new(),
                ],
                vec![String::new()],
            );

            configure_provider_with_io(&home, &workdir, &env_map, &mut io)
                .await
                .expect("setup");

            let config = fs::read_to_string(home.join("config.toml")).expect("config");
            assert!(config.contains(&format!("base_url = \"{expected}\"")));
            assert!(config.contains("api_key_env = \"GLM_API_KEY\""));
            assert!(config.contains("model = \"zai/glm-5.2\""));
            assert!(io.output.contains("Could not fetch models: GLM_API_KEY"));
        }
    }

    #[tokio::test]
    async fn setup_allows_explicit_api_key_env_override() {
        let (_temp, home, workdir, env_map) = setup_workspace();
        let mut io = TestIo::new(
            vec![
                "2".to_string(),
                String::new(),
                "y".to_string(),
                "CUSTOM_API_KEY".to_string(),
                String::new(),
            ],
            vec![String::new()],
        );

        configure_provider_with_io(&home, &workdir, &env_map, &mut io)
            .await
            .expect("setup");

        let config = fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(config.contains("api_key_env = \"CUSTOM_API_KEY\""));
        assert!(config.contains("model = \"zai/glm-5.2\""));
        assert!(io.output.contains("env var name [GLM_API_KEY]: "));
        assert!(io.output.contains("Could not fetch models: CUSTOM_API_KEY"));
    }

    #[tokio::test]
    async fn setup_rejects_pasted_api_key_as_env_var_without_echoing_it() {
        let (_temp, home, workdir, env_map) = setup_workspace();
        let pasted_key = "sk-pasted-secret-value";
        let mut io = TestIo::new(
            vec![
                "1".to_string(),
                unused_loopback_base_url(),
                "y".to_string(),
                pasted_key.to_string(),
                String::new(),
                "manual-model".to_string(),
            ],
            vec!["secret-key".to_string()],
        );

        configure_provider_with_io(&home, &workdir, &env_map, &mut io)
            .await
            .expect("setup");

        let config = fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(config.contains("api_key_env = \"DEEPSEEK_API_KEY\""));
        assert!(!config.contains(pasted_key));
        assert!(!io.output.contains(pasted_key));
        assert!(
            io.output
                .contains("That looks like an API key; enter the key at the hidden prompt next.")
        );
    }

    #[tokio::test]
    async fn xiaomi_token_plan_setup_selects_region_with_canonical_provider_id() {
        let (_temp, home, workdir, env_map) = setup_workspace();
        let mut io = TestIo::new(
            vec![
                "3".to_string(),
                "2".to_string(),
                String::new(),
                String::new(),
            ],
            vec![String::new()],
        );

        configure_provider_with_io(&home, &workdir, &env_map, &mut io)
            .await
            .expect("setup");

        let config = fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(config.contains("xiaomi-token-plan"));
        assert!(config.contains("base_url = \"https://token-plan-sgp.xiaomimimo.com/v1\""));
        assert!(config.contains("api_key_env = \"XIAOMI_TOKEN_PLAN_API_KEY\""));
        assert!(config.contains("model = \"xiaomi-token-plan/mimo-v2.5-pro\""));
    }

    #[tokio::test]
    async fn setup_falls_back_to_custom_model_id_when_fetch_fails() {
        let (_temp, home, workdir, env_map) = setup_workspace();
        let base_url = unused_loopback_base_url();
        let mut io = TestIo::new(
            vec![
                "1".to_string(),
                base_url,
                String::new(),
                "manual-model".to_string(),
            ],
            vec!["secret-key".to_string()],
        );

        configure_provider_with_io(&home, &workdir, &env_map, &mut io)
            .await
            .expect("setup");

        let config = fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(config.contains("model = \"deepseek/manual-model\""));
        assert!(io.output.contains("Could not fetch models:"));
    }

    #[tokio::test]
    async fn custom_provider_path_fetches_empty_catalog_then_prompts_model_id() {
        let (_temp, home, workdir, env_map) = setup_workspace();
        let server = SetupCatalogServer::new(r#"{"data":[]}"#);
        let mut io = TestIo::new(
            vec![
                "5".to_string(),
                "mock-custom".to_string(),
                server.base_url.clone(),
                String::new(),
                "custom-model".to_string(),
            ],
            vec![String::new()],
        );

        configure_provider_with_io(&home, &workdir, &env_map, &mut io)
            .await
            .expect("setup");

        let config = fs::read_to_string(home.join("config.toml")).expect("config");
        assert!(config.contains("mock-custom"));
        assert!(config.contains(&format!("base_url = \"{}\"", server.base_url)));
        assert!(config.contains("api_key_env = \"MOCK_CUSTOM_API_KEY\""));
        assert!(config.contains("model = \"mock-custom/custom-model\""));
        assert!(io.output.contains("No models returned"));
        let requests = server.requests.lock().expect("requests");
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].to_lowercase().contains("authorization:"));
    }
}
