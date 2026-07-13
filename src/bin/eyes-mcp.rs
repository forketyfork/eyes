use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "eyes-mcp",
    about = "MCP server for inspecting and triaging Eyes alerts"
)]
struct Cli {
    #[arg(
        long,
        value_name = "FILE",
        default_value = "eyes.db",
        help = "Path to the Eyes SQLite database"
    )]
    database: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    eyes::mcp::serve(cli.database).await
}
