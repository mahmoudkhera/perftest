use crate::messages_handler::{ServerMessage, read_message, send_message};

use crate::messages_handler::State;
use crate::tcptest::StreamHandle;
use crate::{cli::ClientArgs, messages_handler::ClientMessage};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum Direction {
    /// Traffic flows from Client => Server (the default)
    ClientToServer,
    /// Traffic flows from Server => Client.
    ServerToClient,
    /// Both ways.
    Bidirectional,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct TestParameters {
    pub direction: Direction,
    // omit the first n seconds.
    pub omit_seconds: u32,
    pub time_seconds: u64,
    // The number of data streams
    pub parallel: u16,

    pub block_size: usize,
    pub client_version: String,
    pub no_delay: bool,
    pub socket_buffer: Option<usize>,

    pub mss:Option<i32>


}

impl TestParameters {
    pub fn from_client_args(opts: &ClientArgs, default_block_size: usize) -> Self {
        let direction = if opts.biderctional {
            Direction::Bidirectional
        } else if opts.reverse {
            Direction::ServerToClient
        } else {
            Direction::ClientToServer
        };
        TestParameters {
            direction,
            omit_seconds: 0,
            time_seconds: opts.time,
            parallel: opts.parallel as u16,
            block_size: opts.length.unwrap_or(default_block_size),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            no_delay: opts.nagle,
            socket_buffer: opts.socket_buffers,
            mss:opts.mss
        }
    }
}

pub async fn read_test_parameters(stream: &mut TcpStream) -> Result<TestParameters> {
    match read_message(stream).await? {
        ClientMessage::SendParameters(params) => Ok(params),
        e => Err(anyhow!(
            "Unexpected message, we expect SendParameters, instead we got {:?}",
            e
        )),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Role {
    Server,
    Client,
}


//this is the test that the controler run in both sides client and server
pub struct Test {
    pub client_addr: Option<String>,

    pub cokie: String,

    pub state: Arc<Mutex<State>>,

    pub socket: TcpStream,

    pub role: Role,
    // The test configuration.
    pub params: TestParameters,
    pub streams: HashMap<usize, StreamHandle>,
    // The number of streams at which we are sending data
    pub num_send_streams: u16,
    pub num_receive_streams: u16,
}

impl Test {
    pub fn new(
        client_addr: Option<String>,
        cokie: String,
        socket: TcpStream,
        role: Role,
        params: TestParameters,
    ) -> Self {
        let (mut send, mut recv) = match params.direction {
            Direction::ClientToServer => (params.parallel, 0),
            Direction::ServerToClient => (0, params.parallel),
            Direction::Bidirectional => (params.parallel, params.parallel),
        };

        if matches!(role, Role::Server) {
            std::mem::swap(&mut send, &mut recv);
        }

        Test {
            client_addr,
            cokie,
            // the start stest at first in the test is test start
            state: Arc::new(Mutex::new(State::TestStart)),
            socket,
            role,
            params,
            streams: HashMap::new(),
            num_send_streams: send,
            num_receive_streams: recv,
        }
    }

    pub async fn set_state(&mut self, state: State) -> Result<()> {
        if self.role == Role::Server {
            send_message(&mut self.socket, ServerMessage::SetState(state.clone())).await?;
        }

        *self.state.lock().await = state;

        // lock is dropped immediately after assignment

        // {
        //     let mut locked_state=self.state.lock().await;
        //     *locked_state=state
        // }
        Ok(())
    }

    pub async fn get_state(&self) -> State {
        self.state.lock().await.clone()
    }
}
