use futures::{pin_mut, select};
use futures_util::{future, stream, Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use log::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::HashMap;

use futures_channel::mpsc;

use tokio_tungstenite::{tungstenite, WebSocketStream};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Request {
    pub id: String,
    pub method: String,
    pub params: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Response {
    pub id: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Notification {
    method: String,
    params: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
    Disconnected,
}

pub async fn handle_messages(
    stream: WebSocketStream<tokio::net::TcpStream>,
) -> (
    mpsc::UnboundedReceiver<anyhow::Result<Message>>,
    mpsc::UnboundedSender<anyhow::Result<Message>>,
) {
    let (write, read) = stream.split();

    let (mut read_tx, read_rx) = mpsc::unbounded::<anyhow::Result<Message>>();
    let (write_tx, write_rx) = mpsc::unbounded::<anyhow::Result<Message>>();

    // Inbound message loop
    tokio::spawn(async move {
        let mut incoming_fut = read
            .map_err(|err| error!("websocket error: {}", err))
            .try_filter(|msg| future::ready(msg.is_text()))
            .map(|msg| msg.unwrap())
            .map(|msg| serde_json::from_str::<Message>(msg.to_text().unwrap()))
            .err_into()
            .map(Ok)
            .forward(read_tx);

        let mut outgoing_fut = write_rx
            .map_ok(|msg| serde_json::to_string(&msg).unwrap())
            .map_ok(|msg| tungstenite::Message::from(msg))
            .map_err(|err| tungstenite::error::Error::ConnectionClosed)
            .forward(write);

        select! {
            _ = incoming_fut => info!("websocket closed"),
            _ = outgoing_fut => info!("client dropped websocket"),
        };
    });

    (read_rx, write_tx)
}
