use crate::test_utils::TestParameters;
use anyhow::{Context, Result, bail};
use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize,de::DeserializeOwned};
use std::collections::HashMap;
use std::fmt::Debug;
use thiserror::Error;


use tokio::io::{AsyncReadExt, AsyncWriteExt};
const MESSAGE_LENGTH_SIZE: usize = 4;
const MAX_CONTROL_MESSAGE: u32 = 1024 * 1024 * 20;
const MAX_ALLOWED_SIZE: usize = u32::MAX as usize;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct StreamData {
    pub sender: bool,
    pub duration_millis: u64,
    pub bytes_transferred: usize,
    pub syscalls: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientEnvelope {
    ClientMessage(ClientMessage),
    Error(ErrorControl),
}

/// See docs for `ClientEnvelope`
#[derive(Serialize, Deserialize, Debug)]
pub enum ServerEnvelope {
    ServerMessage(ServerMessage),
    Error(ErrorControl),
}

/// Error messages set by server.
#[derive(Serialize, Deserialize, Debug, Error, Eq, PartialEq)]
pub enum ErrorControl {
    #[error("Access denied: {0}")]
    AccessDenied(String),
    #[error("Cannot accept a stream connection: {0}")]
    CannotAcceptStream(String),

    #[error("Cannot create a stream connection: {0}")]
    CannotCreateStream(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct TestResults {
    pub streams: HashMap<usize, StreamData>,
}

/// A control message that can be sent from clients.
/// CLIENT => SERVER
#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMessage {
    /// The first message that the client needs to send to the server
    /// upon successful connection. The cookie is a random UUID that
    /// the client uses to identify itself and the subsequent stream
    /// connections.
    Hello { cookie: String },
    /// Sending the test parameters
    SendParameters(TestParameters),
    /// Sending the test results
    SendResults(TestResults),
}

/// A control message that can be sent from servers.
/// SERVER => CLIENT
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum ServerMessage {
    /// The server's response to Hello.
    Welcome,
    WelcomeDataStream,
    SetState(State),
    SendResults(TestResults),
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub enum State {
    // Parameters have been exchanged, but server is not ready yet to ask for data stream
    // connections.
    TestStart,
    // Asks the client to establish the data stream connections.
    CreateStreams { cookie: String },
    // All connections are established, stream the data and measure.
    Running,
    // We are asked to exchange the TestResults between server and client. Client will initiate this
    // exchange once it receives a transition into this state.
    ExchangeResults,

    DisplayResults,
}

// send and recives messgaes

pub async fn send_message<A, T>(stream: &mut A, message: T) -> Result<()>
where
    A: AsyncWriteExt + Unpin,
    T: Serialize + Debug,
{
    println!("the sent message{:?}", message);
    let payload = serde_json::to_vec(&message)?; // More efficient than to_string + as_bytes
    let payload_len = payload.len();

    if payload_len > MAX_ALLOWED_SIZE {
        bail!(
            "Message too large: {} bytes (max: {})",
            payload_len,
            MAX_ALLOWED_SIZE
        );
    }

    // Preallocate buffer: 4 bytes for length + payload
    let mut buf = BytesMut::with_capacity(MESSAGE_LENGTH_SIZE + payload_len);
    buf.put_u32(payload_len as u32); // Write length prefix
    buf.extend_from_slice(&payload); // Write payload

    stream.write_all(&buf).await?;
    Ok(())
}

pub async fn read_message<A, T>(stream: &mut A) -> Result<T>
where
    A: AsyncReadExt + Unpin,
    T: DeserializeOwned,
{
    println!("try to read");
    let message_size = stream.read_u32().await?;
    if message_size > MAX_CONTROL_MESSAGE {
        bail!(
            "Unusually large protocol negotiation header: {}MB, max allowed: {}MB",
            message_size / 1024,
            MAX_CONTROL_MESSAGE
        );
    }

    let mut buf = BytesMut::with_capacity(message_size as usize);
    // Ensures we fill `buf` up to `message_size`
    buf.resize(message_size as usize, 0);

    stream.read_exact(&mut buf).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to read full message: {} bytes. Error: {}",
            message_size,
            e
        )
    })?;
    let obj = serde_json::from_slice(&buf)
        .with_context(|| "Invalid protocol, could not deserialize JSON")?;

    Ok(obj)
}
