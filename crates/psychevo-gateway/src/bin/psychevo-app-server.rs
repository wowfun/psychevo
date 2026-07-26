use std::net::SocketAddr;
use std::path::PathBuf;

use psychevo::Application;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> psychevo::Result<()> {
    let options = options_from_args(std::env::args_os().skip(1))?;
    let home = options.home.unwrap_or_else(default_home);
    let mut builder = Application::builder().home(&home);
    if let Some(database_path) = options.database_path {
        builder = builder.database_path(database_path);
    }
    if let Some(config_path) = options.config_path {
        builder = builder.config_path(config_path);
    }
    let application = builder.build().await?;
    match options.listen {
        Some(address) => {
            let token = options.token.ok_or_else(|| {
                psychevo::Error::Message(
                    "--listen requires an explicit non-empty --token".to_string(),
                )
            })?;
            let server =
                psychevo_gateway::app_server::bind_websocket(application, address, token).await?;
            eprintln!("Psychevo App Server listening at {}", server.uri());
            tokio::signal::ctrl_c().await?;
            server.shutdown().await
        }
        None => {
            if options.token.is_some() {
                return Err(psychevo::Error::Message(
                    "--token is valid only with --listen".to_string(),
                ));
            }
            psychevo_gateway::app_server::run_stdio(application).await
        }
    }
}

#[derive(Debug, Default)]
struct Options {
    home: Option<PathBuf>,
    database_path: Option<PathBuf>,
    config_path: Option<PathBuf>,
    listen: Option<SocketAddr>,
    token: Option<String>,
}

fn options_from_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> psychevo::Result<Options> {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or_else(|| {
            psychevo::Error::Message(format!("{} requires a value", arg.to_string_lossy()))
        })?;
        match arg.to_string_lossy().as_ref() {
            "--home" => options.home = Some(value.into()),
            "--database" => options.database_path = Some(value.into()),
            "--config" => options.config_path = Some(value.into()),
            "--listen" => {
                options.listen = Some(value.to_string_lossy().parse().map_err(|error| {
                    psychevo::Error::Message(format!(
                        "invalid --listen address `{}`: {error}",
                        value.to_string_lossy()
                    ))
                })?)
            }
            "--token" => options.token = Some(value.to_string_lossy().into_owned()),
            unknown => {
                return Err(psychevo::Error::Message(format!(
                    "unknown App Server argument: {unknown}"
                )));
            }
        }
    }
    Ok(options)
}

fn default_home() -> PathBuf {
    std::env::var_os("PSYCHEVO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(PathBuf::from)
                .map(|home| home.join(".psychevo"))
        })
        .unwrap_or_else(|| PathBuf::from(".psychevo"))
}
