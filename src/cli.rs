use clap::{ArgGroup, Parser};

#[derive(Debug, Parser)]
pub struct ServerArgs {
    ///server mode
    #[clap(short, long, group = "server_or_client")]
    pub server: bool,
}

#[derive(Parser, Debug)]
pub struct ClientArgs {
    ///client mode
    #[clap(short, long, group = "server_or_client")]
    pub client: Option<String>,

    ///buffer size , default is 2MiB (2097152)
    #[clap(short, long)]
    pub length: Option<usize>,

    /// parallel streams
       #[clap(short = 'P', long, default_value = "1")]

    pub parallel: u32,

    ///transimtion time   default 10s
    #[clap(short, long, default_value = "10")]
    pub time: u64,
    #[clap(short,long, group = "direction")]
    pub biderctional: bool,
    /// Run in reverse mode (server sends, client receives)
    #[clap(short = 'R', long, group = "direction")]
    pub reverse: bool,
    /// SO_SNDBUF/SO_RECVBUF for the data streams, uses the system default if unset.
    #[clap(long)]
    pub socket_buffers: Option<usize>,

    /// disable nagle's algorithm   (read about nagle's algorithm and  how it delays the tcp stream )
    #[clap(short, long)]
    pub  nagle: bool,

    /// set the mss of tcp this only can be change in unix 
    #[clap(short, long)]
    pub mss:Option<i32>,  
    // #[clap(short,long)]
    // pub udp:bool
    //add new (features)
}

#[derive(Parser, Debug)]
pub struct Commands {
    ///server port number``
        #[clap(short, long, default_value = "7559")]
    pub port: u16,
    /// Seconds between periodic throughput reports
    #[clap(short, long, default_value = "1")]
    pub interval: u16,
    //add the required commands
}
#[derive( Debug, Parser)]
#[clap(name="testperf", groups = [
    ArgGroup::new("server_or_client").required(true)])
    
    
    ]
/// a tool for measuring the performance of a network
pub struct Cli{
    #[clap(flatten)]
    pub server_opts: ServerArgs,
    #[clap(flatten)]
    pub client_opts: ClientArgs,
    #[clap(flatten)]
    pub common_commands: Commands,
}
