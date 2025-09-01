use crate::cli::Commands;
use crate::controler::{Controller, ControllerMessage};
use crate::messages_handler::{read_message, send_message, ClientMessage, ErrorControl, ServerMessage, State};
use crate::test_utils::Test;
use crate::test_utils::{Role, read_test_parameters};
use crate::ui;
use anyhow::{Context, Result, anyhow};
use log::debug;
use std::net::Ipv4Addr;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::sync::mpsc::Sender;
use tokio::time::timeout;
const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);



pub async fn run_server(commands: &Commands) -> Result<()> {
    let listener: TcpListener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, commands.port)).await?;

    let port = commands.port;

    ui::print_server_banner(port);

    let mut server_state = ServerState::new();

    while let Ok((control_socket, _)) = listener.accept().await {
        let peer = peer_to_string(&control_socket);

        if let Err(e) = server_state
            .handle_connection(control_socket, &peer, port)
            .await
        {
            println!("[{}] Connection handling failed: {}", peer, e);
        }
    }

    Ok(())
}

struct ServerState {
    in_flight_test: Weak<Mutex<State>>,

    //this send message to the controler that run in another thread
    server_sender: Option<Sender<ControllerMessage>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            in_flight_test: Weak::new(),
            server_sender: None,
        }
    }

    async fn handle_connection(
        &mut self,
        control_socket: TcpStream,
        peer: &str,
        port: u16,
    ) -> Result<()> {
        // not that  at the first the test in flight is weak at first
        //because there is no test already

        match self.in_flight_test.upgrade() {
            Some(state_lock) => {
                self.handle_existing_test(control_socket, peer, state_lock)
                    .await
            }
            None => self.create_new_test(control_socket, peer, port).await,
        }
    }

    async fn handle_existing_test(
        &mut self,
        data_socket: TcpStream,
        peer: &str,
        state_lock: Arc<Mutex<State>>,
    ) -> Result<()> {
        debug!("handle exist test");

        let state = state_lock.lock().await;
        //after each data stream test the client made it send state creat stream with the cookie
        match &*state {
            State::CreateStreams { cookie } => {
                let _expected_cookie = cookie.to_string();
                //release the mutex
                drop(state);
                self.handle_stream_creation(data_socket, peer, _expected_cookie)
                    .await?;
            }
            _ => {
                //release the mutex
                drop(state);
                self.reject_connection(data_socket, peer, "Test already in-flight")
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_stream_creation(
        &self,
        mut data_socket: TcpStream,
        peer: &str,
        expected_cookie: String,
    ) -> Result<()> {
        //here the  cookie is comming from the client controler that identfiy its data stream
        //that will be used to run the test
        let client_cookie = read_cookie(&mut data_socket).await?;

        if client_cookie == expected_cookie {
            send_message(&mut data_socket, ServerMessage::WelcomeDataStream).await?;

            if let Some(ref controller) = self.server_sender {
                controller.try_send(ControllerMessage::CreateStream(data_socket))?;
            }
        } else {
            self.reject_connection(data_socket, peer, "Invalid authentication cookie")
                .await?;
        }

        Ok(())
    }

    async fn create_new_test(
        &mut self,
        control_socket: TcpStream,
        peer: &str,
        port: u16,
    ) -> Result<()> {
        debug!("create new test");

        match create_test(control_socket).await {
            Ok(test) => {
                //downgrading the test state meaning that there is a test
                self.in_flight_test = Arc::downgrade(&test.state);

                let controller = Controller::new(test);

                //make a sender that will send messages to the controller
                self.server_sender = Some(controller.sender.clone());

                self.spawn_controller(controller, port).await;

                Ok(())
            }

            Err(e) => {
                println!("[{}] Failed to create test: {}", peer, e);

                Err(e)
            }
        }
    }

    async fn spawn_controller(&self, controller: Controller, port: u16) {
        tokio::spawn(async move {
            match controller.run_controller().await {
                Ok(_) => println!("Test completed successfully"),
                Err(e) => {
                    println!("Controller aborted: {}", e);
                    println!("Test aborted!");
                }
            }

            //after each test finished we print the banner again
            ui::print_server_banner(port);
        });
    }

    async fn reject_connection(
        &self,
        mut control_socket: TcpStream,
        peer: &str,
        reason: &str,
    ) -> Result<()> {
        println!("Rejecting connection from {}: {}", peer, reason);

        send_message(
            &mut control_socket,
            ErrorControl::AccessDenied(reason.to_owned()),
        )
        .await
        .map_err(|e| e.into())
    }
}

async fn read_cookie(socket: &mut TcpStream) -> Result<String> {
    match read_message(socket).await {
        Ok(ClientMessage::Hello { cookie }) => Ok(cookie),
        Ok(e) => Err(anyhow!(
            "Client sent the wrong welcome message, got: {:?}",
            e
        )),
        Err(e) => Err(anyhow!("Failed to finish initial negotiation: {:?}", e)),
    }
}

async fn create_test(mut control_socket: TcpStream) -> Result<Test> {
    let cookie = read_cookie(&mut control_socket).await?;

    //send a welcom message after we recive the cookie from client hello message
    send_message(&mut control_socket, ServerMessage::Welcome).await?;

    //if client dident recive welcome message or did not send the paramater for any reason
    //the server whait for some time
    let params = timeout(PROTOCOL_TIMEOUT, read_test_parameters(&mut control_socket))
        .await
        .context("Timed out waiting for the protocol negotiation!")??;

    Ok(Test::new(
        None,
        cookie,
        control_socket,
        Role::Server,
        params,
    ))
}
pub fn peer_to_string(control_socket: &TcpStream) -> String {
    control_socket
        .peer_addr()
        .map(|addr| addr.to_string())
        // The reason for or_else here is to avoid allocating the string if this was never called.
        .unwrap_or_else(|_| "<UNKNOWN>".to_owned())
}
