#[derive(Debug, Parser)]
pub(crate) struct GatewayArgs {
    #[command(subcommand)]
    pub(crate) command: Option<GatewayCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum GatewayCommand {
    #[command(about = "Open the managed Gateway Web Shell")]
    Open(GatewayOpenArgs),
    #[command(about = "Start the managed Gateway server without opening a browser")]
    Start(GatewayStartArgs),
    #[command(about = "Configure Gateway messaging channels")]
    Setup(GatewaySetupArgs),
    #[command(about = "Print managed Gateway server status")]
    Status(GatewayStatusArgs),
    #[command(about = "Stop the managed Gateway server")]
    Stop,
    #[command(about = "Restart the managed Gateway server")]
    Restart(GatewayStartArgs),
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct GatewaySetupArgs {
    #[arg(
        long = "channel",
        value_name = "wechat|telegram|feishu|lark",
        help = "Messaging channel to configure"
    )]
    pub(crate) channel: Option<String>,
    #[arg(
        long,
        value_name = "ID",
        help = "Connection id; defaults to the channel"
    )]
    pub(crate) id: Option<String>,
    #[arg(long, value_name = "LABEL", help = "Human display label")]
    pub(crate) label: Option<String>,
    #[arg(
        long = "credential-env",
        value_name = "ENV",
        help = "Environment variable that stores the main channel credential"
    )]
    pub(crate) credential_env: Option<String>,
    #[arg(
        long = "credential-stdin",
        help = "Read the main channel credential from stdin and write profile .env"
    )]
    pub(crate) credential_stdin: bool,
    #[arg(
        long = "qr",
        conflicts_with = "credential_stdin",
        help = "For WeChat, scan an iLink QR code and write bot credentials to profile .env"
    )]
    pub(crate) qr: bool,
    #[arg(
        long = "account-id",
        value_name = "ID",
        help = "For WeChat manual setup, iLink bot account id to write to profile .env"
    )]
    pub(crate) account_id: Option<String>,
    #[arg(
        long = "account-env",
        value_name = "ENV",
        help = "For WeChat, env var that stores the iLink bot account id"
    )]
    pub(crate) account_env: Option<String>,
    #[arg(
        long = "ilink-base-url",
        value_name = "URL",
        help = "For WeChat, iLink API base URL to use and store in profile .env"
    )]
    pub(crate) ilink_base_url: Option<String>,
    #[arg(
        long = "allow-user",
        value_name = "ID",
        help = "Allowed user/operator id; repeatable"
    )]
    pub(crate) allow_users: Vec<String>,
    #[arg(
        long = "allow-group",
        value_name = "ID",
        help = "Allowed group/chat id; repeatable"
    )]
    pub(crate) allow_groups: Vec<String>,
    #[arg(
        long,
        conflicts_with = "disable",
        help = "Enable the channel after setup"
    )]
    pub(crate) enable: bool,
    #[arg(
        long,
        conflicts_with = "enable",
        help = "Disable the channel after setup"
    )]
    pub(crate) disable: bool,
    #[arg(
        long,
        conflicts_with = "restart",
        help = "Start the managed Gateway after setup"
    )]
    pub(crate) start: bool,
    #[arg(
        long,
        conflicts_with = "start",
        help = "Restart the managed Gateway after setup"
    )]
    pub(crate) restart: bool,
    #[arg(long, help = "Emit structured JSON instead of human text")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct GatewayStatusArgs {
    #[arg(long, help = "Emit structured JSON; accepted for script symmetry")]
    pub(crate) json: bool,
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct GatewayOpenArgs {
    #[arg(
        long = "dir",
        value_name = "DIR",
        conflicts_with = "default_workspace",
        help = "Open this cwd in the Web Shell"
    )]
    pub(crate) dir: Option<PathBuf>,
    #[arg(
        long = "default-workspace",
        help = "Open the configured default GUI workspace instead of the current cwd"
    )]
    pub(crate) default_workspace: bool,
    #[arg(
        long,
        value_name = "ADDR",
        help = "Loopback address for a newly started managed Gateway server; omitted uses 127.0.0.1:58080 with managed fallback through 58099"
    )]
    pub(crate) bind: Option<std::net::SocketAddr>,
    #[arg(long, help = "Do not open a browser")]
    pub(crate) no_browser: bool,
    #[arg(long, help = "Include the short-lived launch URL in stdout JSON")]
    pub(crate) print_url: bool,
}

#[derive(Debug, Parser, Clone)]
pub(crate) struct GatewayStartArgs {
    #[arg(
        long,
        value_name = "ADDR",
        help = "Loopback address for the managed Gateway server; omitted uses 127.0.0.1:58080 with managed fallback through 58099"
    )]
    pub(crate) bind: Option<std::net::SocketAddr>,
}
