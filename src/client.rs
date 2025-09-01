use crate::cli::{ClientArgs, Commands};
use crate::messages_handler::{read_message, send_message, ClientMessage, ServerMessage};
use crate::test_utils::{Role, TestParameters};
use crate::test_utils::Test;
pub const MB: usize = 1024 * 1024;
pub const MAX_CONTROL_MESSAGE: u32 = 20 * (MB) as u32;
pub const DEFAULT_BLOCK_SIZE: usize = 2 * MB;
use crate::controler::Controller;
use anyhow::{Context, Ok, Result};
use log::{debug, info};
use tokio::net::TcpStream;
use rand::{distributions::Alphanumeric, Rng};





pub async fn run_client(
    common_opts: &Commands,
    client_opts: &ClientArgs,
) -> Result<(), anyhow::Error> {
    // We are sure this is set at this point.
    let host = client_opts.client.as_ref().unwrap();
    let port = common_opts.port;
    let mut cleint_connection = ClientConnection::build(host, port).await?;

    //set the parameter and but  but system default bcok_size
    let params = TestParameters::from_client_args(client_opts, DEFAULT_BLOCK_SIZE);

    //generate random token for every client connection
    let cookie: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32) // length of token
        .map(char::from)
        .collect();


    //not that this handshake is for make sure that the server we are connecting to 
    //is the wright server and we send the cookie that will identfiy our our id for 
    //every data stream connection because the server will have our cookie
    cleint_connection.handshake(&cookie).await?;

    //send the client parameter to the server
    cleint_connection.send_test_parameters(&params).await?;




    let perf_test = Test::new(
        Some(cleint_connection.address),
        cookie.to_string(),
        cleint_connection.socket,
        Role::Client,
        params,
    );
    debug!("test created");
    let controller = Controller::new(perf_test);
    let handle = tokio::spawn(async move { controller.run_controller().await });
    handle.await??;
    Ok(())
}

struct ClientConnection {
    socket: TcpStream,
    address: String,
}

impl ClientConnection {


    async fn build(host: &str, port: u16) -> Result<Self> {
        let address = format!("{}:{}", host, port);
        debug!("Connecting to {}...", address);

        let socket = TcpStream::connect(&address)
            .await
            .with_context(|| format!("failed to connect to {}", address))?;

        // println!("connected");

        Ok(Self { socket, address })
    }

    async fn handshake(&mut self,cookie:&str) -> Result<()> {
        debug!("Performing handshake...");

        
        send_message(
            &mut self.socket,
            ClientMessage::Hello {
                cookie: cookie.to_string(),
            },
        )
        .await?;

        // wait for the welcom message from the server
        let message: ServerMessage = read_message(&mut self.socket)
            .await
            .context("Failed to receive welcome message or access denied")?;


        debug!("Handshake completed! message is {:?}",message);
        Ok(())
    }

    async fn send_test_parameters(&mut self, params: &TestParameters) -> Result<()> {
        info!("Sending test parameters...");

        send_message(
            &mut self.socket,
            ClientMessage::SendParameters(params.clone()),
        )
        .await
        .context("Failed to send test parameters")?;

        info!("Parameters sent!");
        Ok(())
    }
}
