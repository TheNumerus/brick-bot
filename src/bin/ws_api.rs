use std::time::Duration;

use futures_channel::mpsc::TrySendError;
use serde::Deserialize;

use anyhow::Result;

use futures_util::{future, pin_mut, SinkExt, StreamExt};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::sleep,
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, rx) = futures_channel::mpsc::unbounded();
    //tokio::spawn(read_stdin(stdin_tx));

    let (ws_stream, _ws_res) = connect_async("wss://gateway.discord.gg/?v=8&encoding=json").await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (write, mut read) = ws_stream.split();

    let sender = rx.map(Ok).forward(write);
    tokio::pin!(sender);

    let res = read.next().await.unwrap()?;
    let payload: Payload = serde_json::from_str(&res.to_string())?;

    println!("{:#?}", payload);

    // should have field `d`
    let heartbeat = payload.d.unwrap()["heartbeat_interval"].as_u64().unwrap();

    let hb_tx = tx.clone();

    let heartbeater = tokio::spawn(async move {
        loop {
            let hb_message = json! ({
                "op": 1,
                "d": 0
            });
            hb_tx.unbounded_send(Message::text(hb_message.to_string()))?;
            println!("sent heartbeat");
            sleep(Duration::from_millis(heartbeat)).await;
        }
        Ok::<(), TrySendError<Message>>(())
    });

    println!("Heartbeat: {:#?}", heartbeat);

    // now identify

    let identify = json!({
        "op": 6,
        "d": {
            "token": "Nzg4OTAzMjc4NDA2NjY0MjAz.X9qRbg.DtroTJ9fxQUAXJNti7nLSBS0Q2w",
            "intents": 769,
            "properties": {
                "$os": "windows",
                "$browser": "brick-bot",
                "$device": "brick-bot"
            }
        }
    })
    .to_string();

    tx.unbounded_send(Message::Text(identify.to_string()))?;

    println!("after send");
    //let payload: Payload = serde_json::from_str(&res.to_string())?;

    //println!("{:#?}", payload);

    //write.send_all(&mut Message::Text(identify));
    //let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            tokio::io::stdout().write_all(&data).await.unwrap();
        })
    };

    pin_mut!(ws_to_stdout);

    /*while let Some(msg) = read.next().await {
        println!("{:#?}", msg);
    }*/

    //heartbeater.await?;

    //tokio::select! {_ = sender => {}, _ = heartbeater => {}, _ = ws_to_stdout => {}};
    tokio::join! {sender, heartbeater, ws_to_stdout};
    Ok(())
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
/*async fn read_stdin(tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let mut stdin = tokio::io::stdin();

    let identify = json!({
        "op": 6,
        "d": {
            "token": "Nzg4OTAzMjc4NDA2NjY0MjAz.X9qRbg.DtroTJ9fxQUAXJNti7nLSBS0Q2w",
            "intents": 769,
            "properties": {
                "$os": "windows",
                "$browser": "brick-bot",
                "$device": "brick-bot"
            }
        }
    })
    .to_string();

    tx.unbounded_send(Message::Text(identify)).unwrap();

    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);
        tx.unbounded_send(Message::binary(buf)).unwrap();
    }
}*/

#[derive(Debug, Deserialize)]
struct Payload {
    op: u8,
    d: Option<serde_json::Value>,
    s: Option<usize>,
    t: Option<String>,
}
