//! Browser bindings for Topcoat's iroh connection transport.
#![cfg(target_family = "wasm")]

mod http;
use futures_util::lock::Mutex;
pub use http::*;
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, RecvStream, SendStream, presets},
};
use wasm_bindgen::prelude::*;

const ALPN: &[u8] = b"topcoat/runtime/1";
const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

/// An endpoint whose identity is used to obtain an HTTP transport ticket.
#[wasm_bindgen]
pub struct BrowserEndpoint {
    endpoint: Endpoint,
}

/// Creates a browser endpoint. Close it after the connection ends.
///
/// # Errors
///
/// Returns an error if the endpoint cannot bind.
#[wasm_bindgen(js_name = createEndpoint)]
pub async fn create_endpoint() -> Result<BrowserEndpoint, JsValue> {
    Ok(BrowserEndpoint {
        endpoint: Endpoint::bind(presets::N0).await.map_err(js_error)?,
    })
}

#[wasm_bindgen]
impl BrowserEndpoint {
    /// The public key to send in the HTTP bootstrap request.
    #[must_use]
    pub fn id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Opens the runtime stream and sends its single-use ticket.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is invalid or the connection fails.
    pub async fn connect(&self, address: &str, ticket: &str) -> Result<BrowserConnection, JsValue> {
        let address: EndpointAddr = serde_json::from_str(address).map_err(js_error)?;
        let connection = self
            .endpoint
            .connect(address, ALPN)
            .await
            .map_err(js_error)?;
        let (send, receive) = connection.open_bi().await.map_err(js_error)?;
        let client = BrowserConnection {
            connection,
            send: Mutex::new(send),
            receive: Mutex::new(receive),
        };
        client
            .send(&serde_json::json!({ "ticket": ticket }).to_string())
            .await?;
        Ok(client)
    }

    /// Closes the endpoint and all its connections.
    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

/// A reliable stream of length-prefixed UTF-8 runtime messages.
#[wasm_bindgen]
pub struct BrowserConnection {
    connection: Connection,
    send: Mutex<SendStream>,
    receive: Mutex<RecvStream>,
}

#[wasm_bindgen]
impl BrowserConnection {
    /// Sends a message, waiting for flow control before resolving.
    ///
    /// # Errors
    ///
    /// Rejects messages larger than 8 MiB and failed writes.
    pub async fn send(&self, message: &str) -> Result<(), JsValue> {
        if message.len() > MAX_MESSAGE_SIZE {
            return Err(js_error("runtime message exceeds 8 MiB"));
        }
        let length = u32::try_from(message.len()).map_err(js_error)?;
        let mut send = self.send.lock().await;
        send.write_all(&length.to_be_bytes())
            .await
            .map_err(js_error)?;
        send.write_all(message.as_bytes()).await.map_err(js_error)?;
        Ok(())
    }

    /// Receives a complete message, retaining stream framing across reads.
    ///
    /// # Errors
    ///
    /// Rejects oversized frames, invalid UTF-8, and a closed connection.
    pub async fn receive(&self) -> Result<String, JsValue> {
        let mut receive = self.receive.lock().await;
        let mut length = [0; 4];
        receive.read_exact(&mut length).await.map_err(js_error)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_MESSAGE_SIZE {
            return Err(js_error("runtime message exceeds 8 MiB"));
        }
        let mut message = vec![0; length];
        receive.read_exact(&mut message).await.map_err(js_error)?;
        String::from_utf8(message).map_err(js_error)
    }

    /// Ends the connection and interrupts pending reads and writes.
    pub fn close(&self) {
        self.connection.close(0u32.into(), b"runtime session ended");
    }
}
