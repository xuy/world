use clap::Parser;
use std::process::ExitCode;

use world::cli::{Cli, run};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    run(cli).await
}
