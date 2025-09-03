use anyhow::Result;
use clap::Parser;
use perftest::{cli::Cli, client::run_client, server};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Cli::parse();



     let level = match args.common_commands.debug {
        0 => log::LevelFilter::Warn,   // default
        1 => log::LevelFilter::Info,   // -v
        2 => log::LevelFilter::Debug,  // -vv
        _ => log::LevelFilter::Trace,  // -vvv and more
    };

    env_logger::Builder::new()
        .filter_level(level)
        .init();

    if args.server_opts.server {
        server::run_server(&args.common_commands).await
    } else {
        run_client(&args.common_commands, &args.client_opts).await
    }
}
