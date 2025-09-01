use clap::Parser;
use perftest::{cli::Cli, client::run_client, server};
use anyhow::Result;


#[tokio::main]
async fn main()->Result<(), anyhow::Error> {
    let args=Cli::parse();


    if args.server_opts.server{
        server::run_server(&args.common_commands).await
    }
    else {
        run_client(&args.common_commands, &args.client_opts).await
    }
}
