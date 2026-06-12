use crate::config;
use crate::messages;

use rust_messenger::traits;
use rust_messenger::traits::extended::Sender;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

rust_messenger::messenger_id_enum! {
    HandlerId {
        SyncApp = 1,
        AsyncClient = 2,
        AsyncServer = 3,
        SyncResponseHandler = 4,
}
}

/// Upper bound for a single frame, so a corrupt or hostile length prefix
/// cannot trigger an unbounded allocation.
const MAX_FRAME_LEN: usize = 64 * 1024;

/// TCP is a byte stream: a single `read` may return half a message or two
/// messages glued together. Frame every message with a little-endian u32
/// length prefix so both sides can reassemble exact message boundaries.
async fn write_frame(
    socket: &mut tokio::net::TcpStream,
    payload: &[u8],
) -> std::io::Result<()> {
    socket
        .write_all(&(payload.len() as u32).to_le_bytes())
        .await?;
    socket.write_all(payload).await
}

/// Reads one length-prefixed frame; `Ok(None)` means the peer closed the
/// connection cleanly between frames.
async fn read_frame(socket: &mut tokio::net::TcpStream) -> std::io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match socket.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame of {len} bytes exceeds the {MAX_FRAME_LEN} byte limit"),
        ));
    }
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

pub struct SyncApp {}

impl SyncApp {
    pub fn new<W: traits::core::Writer>(config: &config::Config, _: &W) -> Self {
        println!("SyncApp new called with config value \"{}\"", config.value);
        SyncApp {}
    }
}

impl traits::core::Handler for SyncApp {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::SyncApp;

    fn on_start<W: traits::core::Writer>(&mut self, writer: &W) {
        println!("SyncApp on_start called");

        Self::send(&messages::Request { val: 0 }, writer);
    }
}

impl traits::core::Handle<messages::Response> for SyncApp {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::Response, _writer: &W) {
        println!("received messages::Response at SyncApp: {message:?}");
    }
}

pub struct AsyncClient {
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
    addr: String,
}

impl AsyncClient {
    pub fn new<W: traits::core::Writer>(config: &config::Config, _: &W) -> Self {
        AsyncClient {
            runtime: config.runtime.clone(),
            addr: config.addr.clone(),
        }
    }
}

impl traits::core::Handler for AsyncClient {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::AsyncClient;
}

impl traits::core::Handle<messages::Request> for AsyncClient {
    fn handle<W: traits::core::Writer>(&mut self, message: &messages::Request, writer: &W) {
        println!("received messages::Request at AsyncClient: {message:?}");

        let msg = message.clone();
        let wrt = writer.clone();
        let addr = self.addr.clone();

        self.runtime.spawn(async move {
            // The server binds asynchronously, so retry the connect with a
            // small backoff instead of assuming it is already listening.
            let mut socket = None;
            for _ in 0..100 {
                match tokio::net::TcpStream::connect(&addr).await {
                    Ok(s) => {
                        socket = Some(s);
                        break;
                    }
                    Err(_) => tokio::time::sleep(std::time::Duration::from_millis(2)).await,
                }
            }
            // Panics inside detached tasks are silently swallowed when the
            // JoinHandle is dropped, so report errors explicitly instead.
            let Some(mut socket) = socket else {
                eprintln!("AsyncClient: could not connect to {addr}");
                return;
            };

            let msg_buf = match bincode::serde::encode_to_vec(&msg, bincode::config::standard()) {
                Ok(buf) => buf,
                Err(e) => {
                    eprintln!("AsyncClient: serializing request failed: {e}");
                    return;
                }
            };
            if let Err(e) = write_frame(&mut socket, &msg_buf).await {
                eprintln!("AsyncClient: sending request failed: {e}");
                return;
            }

            println!("AsyncClient send message {msg:?} to {addr}");

            match read_frame(&mut socket).await {
                Ok(Some(frame)) => {
                    // parse incoming response
                    let incoming_response = messages::Response::deserialize_from(&frame);

                    println!("received messages::Response at AsyncClient {incoming_response:?} from {addr}");

                    // send response to message bus
                    Self::send(&incoming_response, &wrt);
                }
                Ok(None) => (), // server closed without responding
                Err(e) => eprintln!("AsyncClient: reading response failed: {e}"),
            }
        });
    }
}

pub struct AsyncServer {
    response_channel: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<usize, tokio::sync::oneshot::Sender<messages::Response>>,
        >,
    >,
}

impl AsyncServer {
    pub fn new<W: traits::core::Writer>(config: &config::Config, writer: &W) -> Self {
        let response_map =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

        config.runtime.spawn(AsyncServer::serve(
            config.addr.clone(),
            writer.clone(),
            response_map.clone(),
            config.runtime.clone(),
        ));

        AsyncServer {
            response_channel: response_map,
        }
    }
}

impl traits::core::Handler for AsyncServer {
    type Id = HandlerId;
    const ID: HandlerId = HandlerId::AsyncServer;
}

impl traits::core::Handle<messages::IdWrapper<messages::Response>> for AsyncServer {
    fn handle<W: traits::core::Writer>(
        &mut self,
        message: &messages::IdWrapper<messages::Response>,
        _writer: &W,
    ) {
        println!("received messages::Response at AsyncServer: {message:?}");
        if let Some(tx) = self.response_channel.blocking_lock().remove(&message.id) {
            // The connection task may have ended (client gone); nothing to
            // deliver to in that case.
            let _ = tx.send(message.val.clone());
        }
    }
}

impl AsyncServer {
    async fn serve<W: traits::core::Writer>(
        addr: String,
        writer: W,
        response_map: std::sync::Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<usize, tokio::sync::oneshot::Sender<messages::Response>>,
            >,
        >,
        runtime: std::sync::Arc<tokio::runtime::Runtime>,
    ) {
        // Setup async TCP server. Report failures instead of panicking: a
        // panic in this detached task would be silently swallowed.
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("AsyncServer: binding {addr} failed: {e}");
                return;
            }
        };

        let request_id_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        println!("Server started at {addr}");

        loop {
            let socket = match listener.accept().await {
                Ok((socket, _)) => socket,
                Err(e) => {
                    eprintln!("AsyncServer: accepting a client failed: {e}");
                    continue;
                }
            };

            println!("Accepted new connection at {addr}");

            runtime.spawn(AsyncServer::serve_client(
                socket,
                request_id_counter.clone(),
                writer.clone(),
                response_map.clone(),
            ));
        }
    }

    async fn serve_client<W: traits::core::Writer>(
        mut socket: tokio::net::TcpStream,
        request_counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        writer: W,
        response_map: std::sync::Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<usize, tokio::sync::oneshot::Sender<messages::Response>>,
            >,
        >,
    ) {
        loop {
            let frame = match read_frame(&mut socket).await {
                Ok(Some(frame)) => frame,
                Ok(None) => return, // client closed the connection
                Err(e) => {
                    eprintln!("AsyncServer: reading a request failed: {e}");
                    return;
                }
            };
            let incoming_request = messages::Request::deserialize_from(&frame);

            let request_id = request_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            let (tx, rx) = tokio::sync::oneshot::channel();
            // insert sender for sync handler
            response_map.lock().await.insert(request_id, tx);

            println!("received messages::Request at AsyncServer: {incoming_request:?}");

            // send request to message bus
            let request = messages::IdWrapper::<messages::Request> {
                id: request_id,
                val: incoming_request,
            };
            Self::send(&request, &writer);

            // wait for response from sync handler; the sender disappears if
            // the messenger stops before responding.
            let Ok(response) = rx.await else { return };

            // Serialize response
            let resp_buff =
                match bincode::serde::encode_to_vec(&response, bincode::config::standard()) {
                    Ok(buf) => buf,
                    Err(e) => {
                        eprintln!("AsyncServer: serializing a response failed: {e}");
                        return;
                    }
                };

            println!("Sending response back to client");

            // Copy the data back to socket
            if let Err(e) = write_frame(&mut socket, &resp_buff).await {
                eprintln!("AsyncServer: sending a response failed: {e}");
                return;
            }
        }
    }
}

pub struct SyncRequestHandler {}

impl SyncRequestHandler {
    pub fn new<W: traits::core::Writer>(_config: &config::Config, _writer: &W) -> Self {
        SyncRequestHandler {}
    }
}

impl traits::core::Handler for SyncRequestHandler {
    type Id = HandlerId;
    const ID: Self::Id = HandlerId::SyncResponseHandler;
}

impl traits::core::Handle<messages::IdWrapper<messages::Request>> for SyncRequestHandler {
    fn handle<W: traits::core::Writer>(
        &mut self,
        message: &messages::IdWrapper<messages::Request>,
        writer: &W,
    ) {
        println!("received messages::Request at SyncRequestHandler: {message:?}");

        let response = messages::IdWrapper::<messages::Response> {
            id: message.id,
            val: messages::Response {
                // Widen before adding: val is a u8 from the network, and
                // val + 1 would overflow for val == 255.
                response_val: message.val.val as u16 + 1,
            },
        };
        Self::send(&response, writer);
    }
}
