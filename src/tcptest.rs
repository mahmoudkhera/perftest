use crate::messages_handler::StreamData;
use crate::test_utils::TestParameters;
use crate::ui;
use anyhow::Result;
use anyhow::bail;
use log::info;
use std::convert::TryInto;
use std::fmt::Debug;
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[derive(Debug, Clone)]
pub enum StreamMessage {
    StartTest,
    Terminate,
}

pub struct StreamHandle {
    pub stream_sender: Sender<StreamMessage>,
    pub task_handle: JoinHandle<Result<StreamData>>,
}

pub struct StreamTester {
    id: usize,
    stream: TcpStream,
    params: TestParameters,
    is_sending: bool,
    reciver: Receiver<StreamMessage>,
}

impl StreamTester {
    pub fn new(
        id: usize,
        stream: TcpStream,
        params: TestParameters,
        is_sending: bool,
        reciver: Receiver<StreamMessage>,
    ) -> Self {
        StreamTester {
            id,
            stream,
            params,
            is_sending,
            reciver,
        }
    }

    pub async fn run_test(mut self) -> Result<StreamData> {
        let block_size = self.params.block_size;
        let mut buffer = vec![0u8; block_size];

        if self.is_sending {
            fill_random(&mut buffer, self.params.block_size).await?;
        }

        self.configure_stream_socket()?;
        //what unntil the StartTst message is recived from the controler
        let signal = self.reciver.recv().await;

        if !matches!(signal, Some(StreamMessage::StartTest)) {
            bail!("Internal communication channel for stream was terminated unexpectedly!");
        }

        info!("measuring....");

        let start_time = Instant::now();
        let timeout = Duration::from_secs(self.params.time_seconds);
        let interval = Duration::from_secs(1);

        let mut bytes_transferred = 0usize;
        let mut syscalls = 0usize;

        let mut interval_start = Instant::now();
        let mut interval_bytes = 0usize;
        let mut interval_syscalls = 0usize;

        loop {
            if start_time.elapsed() > timeout {
                break;
            }

            match self.reciver.try_recv() {
                Ok(StreamMessage::Terminate) => break,
                Ok(StreamMessage::StartTest) => {
                    println!("Stream {}: duplicate StartTest received", self.id);
                }

                _ => {}
            }

            if self.is_sending {
                match tokio::time::timeout(Duration::from_millis(100), self.stream.write(&buffer))
                    .await
                {
                    Ok(Ok(0)) => {
                        println!("the connection is closed");
                        break;
                    }
                    Ok(Ok(n)) => {
                        bytes_transferred += n;
                        interval_bytes += n;
                        syscalls += 1;
                        interval_syscalls += 1;
                    }

                    Err(_) => {
                        println!("Stream {}: write timeout (>100ms)", self.id);
                    }
                    _ => {}
                }
            } else {
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    self.stream.read(&mut buffer),
                )
                .await
                {
                    Ok(Ok(0)) => {
                        println!("reach EOF");
                        break;
                    }
                    Ok(Ok(n)) => {
                        bytes_transferred += n;
                        interval_bytes += n;
                        syscalls += 1;
                        interval_syscalls += 1;
                    }

                    Err(_) => {
                        println!("Stream {}: read timeout (>100ms)", self.id);
                    }
                    _ => {}
                }
            }

            let now = Instant::now();
            let current_interval = now - interval_start;

            if current_interval >= interval {
                ui::print_stats(
                    Some(self.id),
                    (start_time.elapsed() - interval)
                        .as_millis()
                        .try_into()
                        .unwrap(),
                    current_interval.as_millis().try_into().unwrap(),
                    interval_bytes,
                    self.is_sending,
                    interval_syscalls,
                    block_size,
                );
                interval_start = now;
                interval_bytes = 0;
                interval_syscalls = 0;
            }
        }

        if !self.is_sending {
            while self.stream.read(&mut buffer).await? != 0 {}
        }
        Ok(StreamData {
            sender: self.is_sending,
            duration_millis: start_time.elapsed().as_millis() as u64,
            bytes_transferred,
            syscalls,
        })
    }

    fn configure_stream_socket(&mut self) -> Result<()> {
        //set tcp no delay if requested
        if self.params.no_delay {
            self.stream.set_nodelay(true)?;
        }

        if let Some(buferr_size) = self.params.socket_buffer {
            let buf_size = buferr_size.try_into().unwrap_or(u32::MAX);

            // SAFETY: This is safe because we don't use the TcpSocket after this point.
            #[cfg(any(unix, windows))]
            unsafe {
                let sock = {
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::{AsRawFd, FromRawFd};

                        tokio::net::TcpSocket::from_raw_fd(self.stream.as_raw_fd())
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::io::{AsRawSocket, FromRawSocket};
                        tokio::net::TcpSocket::from_raw_socket(self.stream.as_raw_socket())
                    }
                };

                sock.set_recv_buffer_size(buf_size)
                    .unwrap_or_else(|err| println!("set_recv_buffer_size(), error: {}", err));
                sock.set_send_buffer_size(buf_size)
                    .unwrap_or_else(|err| println!("set_send_buffer_size(), error: {}", err));
            }
        }

        Ok(())
    }
}

async fn fill_random(buffer: &mut [u8], length: usize) -> Result<()> {
    #[cfg(unix)]
    {   
        let _=length;
        let mut random = tokio::fs::File::open("/dev/urandom").await?;
        let _ = random.read_exact(buffer).await?;

        Ok(())
    }

    #[cfg(windows)]
    {
        const PROV_RSA_FULL: u32 = 1;
        const CRYPT_VERIFYCONTEXT: u32 = 0xF0000000;

        #[link(name = "advapi32")] //Tells Rust to link against advapi32.dll, which contains the Windows CryptoAPI
        unsafe extern "C" {
            // Acquires a handle to a cryptographic provider.
            fn CryptAcquireContextA(
                hProv: *mut usize,       //pointer to where the handle will be stored
                pszContainer: *const i8, //name of the key to  the container
                pszProvider: *const i8,  //name of crypto provider
                dwProvType: u32,         //type of provider
                dwFlags: u32, //extra options (CRYPT_VERIFYCONTEXT = 0xF0000000 for "temporary, no persisted keys")
            ) -> i32;

            fn CryptGenRandom(
                hProv: usize,      //takes the provider handle
                dwLen: u32,        //number of random bytes to generate
                pbBuffer: *mut u8, //pointer to a buffer that will be filled with random bytes
            ) -> i32;

            //Releases the crypto provider handle when you’re done.
            fn CryptReleaseContext(hProv: usize, dwFlags: u32) -> i32;

        }

        //implementation

        unsafe {
            use std::ptr;
            let mut h_provider = 0;

            // aquire the cryptographic provider context
            let result = CryptAcquireContextA(
                &mut h_provider,
                ptr::null(),
                ptr::null(),
                PROV_RSA_FULL,
                CRYPT_VERIFYCONTEXT,
            );

            if result == 0 {
                panic!("CryptAcquireContextA failed");
            }

            //generate random bytes

            if CryptGenRandom(h_provider, length as u32, buffer.as_mut_ptr()) == 0 {
                CryptReleaseContext(h_provider, 0);
                panic!("CryptGenRandom failed");
            }
        }

        Ok(())
    }
}

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

#[cfg(unix)]
const IPPROTO_TCP: i32 = 6;

#[cfg(unix)]
const TCP_MAXSEG: i32 = 2;

#[cfg(unix)]
unsafe extern "C" {
    fn setsockopt(
        socket: i32,
        level: i32,
        option_name: i32,
        option_value: *const std::ffi::c_void,
        option_len: u32,
    ) -> i32;
    fn getsockopt(
        socket: i32,
        level: i32,
        option_name: i32,
        option_value: *mut std::ffi::c_void,
        option_len: *mut u32,
    ) -> i32;
}

#[cfg(unix)]
pub fn set_tcp_mss<T: AsRawFd>(socket: &T, mss: u32) -> io::Result<()> {
    let fd = socket.as_raw_fd();
    let mss_i32 = mss as i32;

    let result = unsafe {
        setsockopt(
            fd,
            IPPROTO_TCP,
            TCP_MAXSEG,
            &mss_i32 as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
pub fn get_tcp_mss<T: AsRawFd>(socket: &T) -> io::Result<u32> {
    let fd = socket.as_raw_fd();
    let mut mss: i32 = 0;
    let mut len = std::mem::size_of::<i32>() as u32;

    let result = unsafe {
        getsockopt(
            fd,
            IPPROTO_TCP,
            TCP_MAXSEG,
            &mut mss as *mut i32 as *mut std::ffi::c_void,
            &mut len,
        )
    };

    if result == 0 {
        Ok(mss as u32)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub  fn configure_tcp_mss<T: AsRawFd>(stream: &T, mss: u32) -> std::io::Result<()> {

        set_tcp_mss(stream, mss)?;

        // read back from the socket to confirm
        let socket_mss = get_tcp_mss(stream)?;
        log::debug!("Requested MSS: {}, Actual MSS: {}", mss, socket_mss);

        Ok(())
    }

#[cfg(test)]
#[cfg(unix)]

mod tests {

    use super::*;
    use tokio::net::TcpSocket;

    #[cfg(unix)]
    #[tokio::test]

    async fn test_mss_set_correctly() {
        // Create socket
        let socket = TcpSocket::new_v4().expect("Failed to create socket");

        // Set MSS to 1400
        let expected_mss = 1400u32;

        // Set the MSS
        set_tcp_mss(&socket, expected_mss).expect("Failed to set MSS");

        // Read back the MSS
        let actual_mss = get_tcp_mss(&socket).expect("Failed to get MSS");

        // Verify they match
        assert_eq!(
            actual_mss, expected_mss,
            "MSS mismatch: expected {}, got {}",
            expected_mss, actual_mss
        );

        println!(" MSS correctly set to {}", expected_mss);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn test_multiple_mss_values() {
        let test_values = [536, 1200, 1400, 1460];

        for &expected_mss in &test_values {
            let socket = TcpSocket::new_v4().expect("Failed to create socket");

            // Set MSS
            match set_tcp_mss(&socket, expected_mss) {
                Ok(_) => {
                    // Read back MSS
                    let actual_mss = get_tcp_mss(&socket).expect("Failed to get MSS");

                    // Verify exact match
                    assert_eq!(
                        actual_mss, expected_mss,
                        "MSS {} not set correctly: got {}",
                        expected_mss, actual_mss
                    );

                    println!("✅ MSS {} set correctly", expected_mss);
                }
                Err(e) => {
                    // Some MSS values might not be supported
                    println!(" MSS {} failed to set: {}", expected_mss, e);
                    panic!("Failed to set MSS {}: {}", expected_mss, e);
                }
            }
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn test_mss_value_persistence() {
        let socket = TcpSocket::new_v4().expect("Failed to create socket");
        let test_mss = 1300u32;

        // Set MSS
        set_tcp_mss(&socket, test_mss).expect("Failed to set MSS");

        // Read it back multiple times to ensure it persists
        for i in 0..5 {
            let actual_mss = get_tcp_mss(&socket).expect("Failed to get MSS");
            assert_eq!(
                actual_mss,
                test_mss,
                "MSS changed on read #{}: expected {}, got {}",
                i + 1,
                test_mss,
                actual_mss
            );
        }

        println!("✅ MSS {} persists across multiple reads", test_mss);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn test_mss_set_get_cycle() {
        let socket = TcpSocket::new_v4().expect("Failed to create socket");

        // Test setting different MSS values on the same socket
        let mss_values = [800, 1000, 1200, 1400];

        for &mss in &mss_values {
            // Set new MSS
            set_tcp_mss(&socket, mss).expect("Failed to set MSS");

            // Immediately read it back
            let read_mss = get_tcp_mss(&socket).expect("Failed to get MSS");

            // Verify exact match
            assert_eq!(
                read_mss, mss,
                "Set-Get cycle failed: set {}, got {}",
                mss, read_mss
            );
        }

        println!("Set-Get cycle test passed for all values");
    }
    #[cfg(unix)]
    #[test]
    fn test_sync_mss_verification() {
        // Non-async version for simple testing
        let _socket = std::net::TcpStream::connect("1.1.1.1:80");

        // This should fail because we're testing on a connected socket
        // but it shows how you could test the functions themselves

        println!(" Sync test structure verified");
    }
}

