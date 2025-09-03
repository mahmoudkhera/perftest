
// this feature not  add yet but comming soon





const HEADER_SIZE: usize = 8 + 8 + 8 + 4; // 28 bytes
const FLAG_DATA: u32 = 0;
const FLAG_FIN: u32 = 1;

struct UdpData {
    start: Instant,
    first_rx_set: bool,
    last_seq: Option<u64>,
    received: u64,
    lost: u64,
    ooo: u64,
    bytes: u64,
    jitter_ms: f64,
    prev_transit_ms: Option<f64>,
    last_report: Instant,
}
pub struct UdpTester {
    id: usize,
    udp_socket: UdpSocket,
    params: TestParameters,
    is_sending: bool,
    reciver: Receiver<StreamMessage>,
}
pub struct UdpHandle {
    pub udp_stream_sender: Sender<StreamMessage>,
    pub task_handle: JoinHandle<Result<UdpData>>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct UdpHeader {
    seq: u64,   //  packet   sequence number
    sec: u64,   //  seconds  since unix_epoch
    usec: u64,  //  micro seconds,
    flags: u32, //0 =data  ,1 =FIN the end of the test
}

// helper functions
fn write_header(buffer: &mut [u8], header: &UdpHeader) {
    assert!(buffer.len() >= HEADER_SIZE);

    buffer[0..8].copy_from_slice(&header.seq.to_be_bytes());
    buffer[8..16].copy_from_slice(&header.sec.to_be_bytes());
    buffer[16..24].copy_from_slice(&header.usec.to_be_bytes());
    buffer[24..28].copy_from_slice(&header.flags.to_be_bytes());
}

fn read_header(buffer: &mut [u8]) -> UdpHeader {
    let seq = u64::from_be_bytes(buffer[0..8].try_into().unwrap());
    let sec = u64::from_be_bytes(buffer[8..16].try_into().unwrap());
    let usec = u64::from_be_bytes(buffer[16..24].try_into().unwrap());
    let flags = u32::from_be_bytes(buffer[24..28].try_into().unwrap());
    UdpHeader {
        seq,
        sec,
        usec,
        flags,
    }
}

impl UdpTester {
    pub fn new(
        id: usize,
        udp_socket: UdpSocket,
        params: TestParameters,
        is_sending: bool,
        reciver: Receiver<StreamMessage>,
    ) -> Self {
        UdpTester {
            id,
            udp_socket,
            params,
            is_sending,
            reciver,
        }
    }

    async fn run_client(&mut self) -> Result<UdpData> {
        let block_size = self.params.block_size;
        let mut buffer = vec![0u8; block_size];

        if self.is_sending {
            fill_random(&mut buffer, self.is_sending);

            // write_herader(buffer, header);
        }

        //put it hard coded for now
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.connect("127.0.0.1:7559")?;

        let payload_size = 1200usize;
        // payload_size = size of the UDP packet payload (in bytes).
        let bits_per_packet = (payload_size * 8) as f64;

        // bitrate_bps = desired sending rate in bits per second (bps).
        let bitrate_bps = 10_000_000;
        //pps is number of packets sent per second
        let pps = (bitrate_bps as f64 / bits_per_packet).max(1.0);

        //time between every packet
        let interval_per_pkt = Duration::from_secs_f64(1.0 / pps);

        let start = Instant::now();
        let mut last_report = Instant::now();
        let mut seq: u64 = 0;
        let timeout = Duration::from_secs(self.params.time_seconds);

        while start.elapsed() < timeout {
            let (sec, usec) = now_micros();
            let hdr = UdpHeader {
                seq,
                sec,
                usec,
                flags: FLAG_DATA,
            };
            let mut buf = vec![0u8; payload_size];
            write_header(&mut buf[..HEADER_SIZE], &hdr);
            // payload bytes are left as zeros
            let _ = sock.send(&buf)?;
            seq += 1;
        }

        todo!()
    }
}

fn now_micros() -> (u64, u64) {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    (d.as_secs(), d.subsec_micros() as u64)
}
