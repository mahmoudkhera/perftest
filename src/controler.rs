use std::collections::HashMap;
use std::time::Duration;

use crate::messages_handler::{
    ClientMessage, ServerMessage, TestResults, read_message, send_message,
};
use crate::tcptest::{StreamHandle, StreamMessage, StreamTester};
use crate::test_utils::{Direction, Role};
use crate::ui;
use crate::{
    messages_handler::{State, StreamData},
    test_utils::Test,
};
use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use tokio::net::{TcpSocket, TcpStream};
use tokio::select;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::time::timeout;

const CHANNEL_BUFF: usize = 5;

#[derive(Debug)]
//this controler messages used to  ccomunciaet with  with server
pub enum ControllerMessage {
    CreateStream(TcpStream),
    StreamTerminated(usize),
}

pub struct Controller {
    pub sender: Sender<ControllerMessage>,
    test: Test,
    receiver: Receiver<ControllerMessage>,
    stream_results: HashMap<usize, StreamData>,
}

impl Controller {
    pub fn new(test: Test) -> Self {
        let (sender, receiver) = mpsc::channel(5);

        Self {
            test,
            sender,
            receiver,
            stream_results: HashMap::new(),
        }
    }

    // Main controller loop
    pub async fn run_controller(mut self) -> Result<()> {
        warn!("Controller has started");

        loop {
            info!(
                "sync state from test state {:?}",
                self.test.get_state().await
            );

            // Handle state transitions
            if let Some(should_break) = self.handle_state().await? {
                if should_break {
                    break;
                }
            }
            warn!("state after handle it is {:?}", self.test.get_state().await);

            // Process messages based on role
            match self.test.role {
                Role::Server => self.handle_server_messages().await?,
                Role::Client => self.handle_client_messages().await?,
            }
        }

        println!("test finished");
        Ok(())
    }

    async fn handle_state(&mut self) -> Result<Option<bool>> {
        debug!("Controller state: {:?}", self.test.get_state().await);

        match (&self.test.get_state().await, &self.test.role) {
            (State::TestStart, Role::Server) => {
                info!("Test is initializing");
                self.test
                    .set_state(State::CreateStreams {
                        cookie: self.test.cokie.clone(),
                    })
                    .await?;
            }
            // not that this match arm match will not match
            //if the there is only on test stream because
            //it will transit immediately in the accept function
            (State::CreateStreams { .. }, Role::Server) => {
                debug!("Waiting for data streams to be connected");
            }
            (State::CreateStreams { .. }, Role::Client) => {
                debug!("Creating data streams");

                self.create_streams().await?;
            }
            (State::Running, _) => {
                if !self.test.streams.is_empty() {
                    ui::print_header();
                    self.broadcast_to_streams(StreamMessage::StartTest);
                }
            }
            (State::ExchangeResults, _) => {
                self.finalize_test().await?;
                return Ok(Some(true)); // Break the loop
            }
            _ => {} // Other states don't require action
        }

        Ok(None)
    }

    async fn handle_server_messages(&mut self) -> Result<()> {
        debug!("handldle server message");
        if let Some(message) = self.receiver.recv().await {
            self.process_internal_message(message).await?;
        }
        Ok(())
    }

    async fn handle_client_messages(&mut self) -> Result<()> {
        debug!("handle_client_messages");
        let internal_message = self.receiver.recv();
        let server_message = read_message(&mut self.test.socket);

        select! {
            message = internal_message => {
                if let Some(msg) = message {
                    self.process_internal_message(msg).await?;
                }
            }
            message = server_message => {
                self.process_server_message(message).await?;
            }
            else => {
                println!("All channels closed, terminating controller");
            }
        }
        Ok(())
    }
    async fn process_internal_message(&mut self, message: ControllerMessage) -> Result<()> {
        debug!("process_internal_message");
        match message {
            ControllerMessage::CreateStream(stream) => {
                self.accept_stream(stream).await?;
            }
            ControllerMessage::StreamTerminated(id) => {
                self.handle_stream_termination(id).await?;
            }
        }
        Ok(())
    }

    async fn handle_stream_termination(&mut self, id: usize) -> Result<()> {
        if let Some(stream_handle) = self.test.streams.remove(&id) {
            match timeout(Duration::from_secs(5), stream_handle.task_handle).await {
                Ok(Ok(Ok(result))) => {
                    self.stream_results.insert(id, result);
                    println!("Stream {} joined successfully", id);
                }
                Ok(Ok(Err(e))) => {
                    println!("Stream {} terminated with error: {}", id, e);
                }
                Ok(Err(e)) => {
                    println!("Failed to join stream {}: {}", id, e);
                }
                Err(_) => {
                    println!("Timeout waiting for stream {} to terminate", id);
                }
            }
        }

        // Check if all streams are done (server only)
        if self.test.streams.is_empty() && self.test.role == Role::Server {
            self.test.set_state(State::ExchangeResults).await?;
        }

        Ok(())
    }

    async fn process_server_message(&mut self, message: Result<ServerMessage>) -> Result<()> {
        let message = message?;
        debug!("process_server_message");

        match message {
            ServerMessage::SetState(state) => {
                self.test.set_state(state).await?;
            }
            msg => {
                println!("Received unexpected message from server: {:?}", msg);
            }
        }
        Ok(())
    }
    async fn finalize_test(&mut self) -> Result<()> {
        info!("Finalizing test and exchanging results");

        // Terminate all streams
        // ask all the streams to terminate
        self.broadcast_to_streams(StreamMessage::Terminate);

        // Collect and exchange results
        let local_results = self.collect_test_results().await?;
        let remote_results = self.exchange_results(local_results.clone()).await?;

        // Print final results
        ui::print_summary(&local_results, &remote_results, &self.test.params.direction);
        Ok(())
    }

    // Stream Management
    async fn create_streams(&mut self) -> Result<()> {
        debug!("start stream createion");

        let total_needed = self.calculate_total_streams();

        for stream_idx in self.test.streams.len()..total_needed {
            debug!("Creating stream {}/{}", stream_idx + 1, total_needed);

            let stream = self
                .connect_data_stream()
                .await
                .with_context(|| format!("Failed to create stream {}", stream_idx))?;
            debug!("done creating the data stream");
            self.register_stream(stream)?;
        }

        Ok(())
    }

    //this function run only in server or there is  bidrectional test
    async fn accept_stream(&mut self, stream: TcpStream) -> Result<()> {
        debug_assert_eq!(self.test.role, Role::Server);

        #[cfg(unix)]
        if let Some(mss) = self.test.params.mss {
            use crate::tcptest;

           tcptest::configure_tcp_mss(&stream, mss as u32)?;
        }

        let total_needed = self.calculate_total_streams();

        let current_count = self.test.streams.len();

        // register the new stream
        //for the new stream test that came fro the client
        self.register_stream(stream)?;

        debug!("Stream accepted ({}/{})", current_count + 1, total_needed);

        // Check if we have all required streams
        if self.test.streams.len() == total_needed {
            println!("All streams connected, transitioning to Running state");
            self.test.set_state(State::Running).await?;
        }

        Ok(())
    }

    async fn connect_data_stream(&mut self) -> Result<TcpStream> {
        let address = self
            .test
            .client_addr
            .as_ref()
            .ok_or_else(|| anyhow!("Client address not set"))?;

        debug!("Opening data stream to {}", address);

        let test_socket = TcpSocket::new_v4()?;
          #[cfg(unix)]
        if let Some(mss) = self.test.params.mss {
            use crate::tcptest;

           tcptest::configure_tcp_mss(&test_socket, mss as u32)?;
        }


        let mut data_stream = test_socket.connect(address.parse().unwrap()).await?;

        // client send the cookie again throug the data stream
        //and whait from the server to check if the data stream is opened
        //this message is sent for every data data
        send_message(
            &mut data_stream,
            ClientMessage::Hello {
                cookie: self.test.cokie.clone(),
            },
        )
        .await?;

        // Wait for welcome message
        let welcome_data_stream: ServerMessage = read_message(&mut data_stream).await?;
        debug!("{:?}", welcome_data_stream);

        Ok(data_stream)
    }

    fn register_stream(&mut self, stream: TcpStream) -> Result<()> {
        //pich the id of the next test
        let stream_id = self.test.streams.len();
        info!("stream id in the {}", stream_id);

        let is_sending = self.determine_stream_direction(stream_id);

        debug!("Creating stream  {} (sending: {})", stream_id, is_sending);

        //intialte the channel for internal messages
        let (sender, receiver) = mpsc::channel(CHANNEL_BUFF);

        let stream_test = StreamTester::new(
            stream_id,
            stream,
            self.test.params.clone(),
            is_sending,
            receiver,
        );

        let thread_sender = self.sender.clone();
        debug!("test start ");
        let handle = tokio::spawn(async move {
            let result = stream_test.run_test().await;

            // Notify controler  that stream test terminated
            if let Err(e) = thread_sender.try_send(ControllerMessage::StreamTerminated(stream_id)) {
                println!("Failed to notify controller of stream termination: {}", e);
            }

            result
        });

        let stream_handle = StreamHandle {
            stream_sender: sender,
            task_handle: handle,
        };

        self.test.streams.insert(stream_id, stream_handle);
        Ok(())
    }

    fn determine_stream_direction(&self, id: usize) -> bool {
        //check if  the last test number is less than total streams sent
        let mut is_sending = id < self.test.num_send_streams as usize;

        // Special case for bidirectional server streams
        if self.test.role == Role::Server && self.test.params.direction == Direction::Bidirectional
        {
            is_sending = !is_sending;
        }

        is_sending
    }

    fn calculate_total_streams(&self) -> usize {
        (self.test.num_send_streams + self.test.num_receive_streams) as usize
    }

    fn broadcast_to_streams(&mut self, message: StreamMessage) {
        debug!(
            "Broadcasting message to {} streams: {:?}",
            self.test.streams.len(),
            message
        );

        for (id, stream_handle) in &self.test.streams {
            if let Err(e) = stream_handle.stream_sender.try_send(message.clone()) {
                println!("Failed to send message to stream {}: {}", id, e);
            }
        }
    }

    async fn collect_test_results(&mut self) -> Result<TestResults> {
        debug!(
            "Collecting results from {} streams",
            self.test.streams.len()
        );

        // Wait for all remaining streams to finish
        for (id, stream_handle) in self.test.streams.drain() {
            match timeout(Duration::from_secs(5), stream_handle.task_handle).await {
                Ok(Ok(Ok(result))) => {
                    //collect the stream handles in the controler from  the test
                    self.stream_results.insert(id, result);
                    debug!("Collected result from stream {}", id);
                }
                Ok(Ok(Err(e))) => {
                    println!("Stream {} failed: {}", id, e);
                }
                Ok(Err(e)) => {
                    println!("Failed to join stream {}: {}", id, e);
                }
                Err(_) => {
                    println!("Timeout collecting result from stream {}", id);
                }
            }
        }

        Ok(TestResults {
            streams: self.stream_results.clone(),
        })
    }

    async fn exchange_results(&mut self, local_results: TestResults) -> Result<TestResults> {
        debug!("Exchanging results with peer");

        match self.test.role {
            Role::Client => self.client_exchange_results(local_results).await,
            Role::Server => self.server_exchange_results(local_results).await,
        }
    }

    async fn client_exchange_results(&mut self, test_results: TestResults) -> Result<TestResults> {
        // Send client results first
        send_message(
            &mut self.test.socket,
            ClientMessage::SendResults(test_results),
        )
        .await?;

        // Read server's results
        match read_message(&mut self.test.socket).await? {
            ServerMessage::SendResults(results) => Ok(results),
            msg => Err(anyhow!("Expected SendResults, got: {:?}", msg)),
        }
    }

    async fn server_exchange_results(&mut self, local_results: TestResults) -> Result<TestResults> {
        // Read client's results first
        let remote_results = match read_message(&mut self.test.socket).await? {
            ClientMessage::SendResults(results) => results,
            msg => return Err(anyhow!("Expected SendResults, got: {:?}", msg)),
        };

        // Send server results
        send_message(
            &mut self.test.socket,
            ServerMessage::SendResults(local_results),
        )
        .await?;

        Ok(remote_results)
    }

    
}
