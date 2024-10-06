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
        std::thread::sleep(std::time::Duration::from_millis(1)); // wait for server to start

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
            let mut socket = tokio::net::TcpStream::connect(&addr)
                .await
                .expect("opening connect failed");

            let msg_buf = bincode::serialize(&msg).expect("Serializing message in client failed");
            socket
                .write_all(&msg_buf)
                .await
                .expect("Writing data in client socket failed");

            println!("AsyncClient send message {msg:?} to {addr}");

            let mut buf = vec![0u8; 1024];

            match socket.read(&mut buf).await {
                Ok(0) => (),
                Ok(n) => {
                    // you could skip the extra parsing & serializing
                    // wrt.write::<messages::Response, AsyncClient, _>(n, |buf2| {
                    //     buf2.copy_from_slice(&buf[..n])
                    // });

                    // parse incoming response
                    let incoming_response = messages::Response::deserialize_from(&buf[..n]);

                    println!("received messages::Response at AsyncClient {incoming_response:?} from {addr}");

                    // send response to message bus
                    Self::send(&incoming_response, &wrt);
                }
                Err(_) => {
                    // Unexpected socket error. There isn't much we can do
                    // here so just stop processing.
                }
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
            tx.send(message.val.clone())
                .expect("One shot received was closed...");
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
        // Setup async TCP server
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("Error binding server");

        let request_id_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        println!("Server started at {addr}");

        loop {
            let (socket, _) = listener.accept().await.expect("error accepting new client");

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
        let mut buf = vec![0; 1024];

        loop {
            match socket.read(&mut buf).await {
                Ok(0) => return,
                Ok(n) => {
                    let incoming_request = messages::Request::deserialize_from(&buf[..n]);

                    let request_id =
                        request_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

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

                    // wait for response from sync handler
                    let response = rx.await.expect("The sender dropped!");

                    // Serialize response
                    let resp_buff =
                        bincode::serialize(&response).expect("Serializing of response failed");

                    println!("Sending response back to client");

                    // Copy the data back to socket
                    if socket.write_all(&resp_buff).await.is_err() {
                        // Unexpected socket error. There isn't much we can
                        // do here so just stop processing.
                        return;
                    }
                }
                Err(e) => {
                    // Unexpected socket error. There isn't much we can do
                    // here so just stop processing.
                    eprintln!("Error in socket {e}");
                    return;
                }
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
                response_val: (message.val.val + 1) as u16,
            },
        };
        Self::send(&response, writer);
    }
}
