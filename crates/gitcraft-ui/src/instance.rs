//! One window per user: a second launch hands its repository to the first and exits.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use iced::futures::{SinkExt, Stream};
use tokio::io::AsyncReadExt;

/// Only requests starting with this are accepted, so a stray connection does nothing.
const GREETING: &str = "gitcraft-open\n";

fn port_file() -> PathBuf {
    gitcraft_core::store::config_dir().join("instance.port")
}

/// Asks a running instance to open `repo`. True if one answered, so this one can exit.
pub fn forward_to_running(repo: Option<&str>) -> bool {
    let Some(port) = std::fs::read_to_string(port_file())
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
    else {
        return false;
    };
    let address = (Ipv4Addr::LOCALHOST, port).into();
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let path = repo.map(gitcraft_core::paths::expand).unwrap_or_default();
    if stream
        .write_all(format!("{GREETING}{path}").as_bytes())
        .is_err()
        || stream.shutdown(std::net::Shutdown::Write).is_err()
    {
        return false;
    }
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).is_ok() && &reply == b"ok"
}

/// Repository paths sent by later launches.
pub fn listen() -> impl Stream<Item = String> {
    iced::stream::channel(8, async move |mut output| {
        let Ok(listener) = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await else {
            return;
        };
        let Ok(address) = listener.local_addr() else {
            return;
        };
        let file = port_file();
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&file, address.port().to_string()).is_err() {
            return;
        }
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut text = String::new();
            let reader = stream.read_to_string(&mut text);
            if tokio::time::timeout(Duration::from_secs(1), reader)
                .await
                .is_err()
            {
                continue;
            }
            let Some(path) = text.strip_prefix(GREETING) else {
                continue;
            };
            use tokio::io::AsyncWriteExt as _;
            let _ = stream.write_all(b"ok").await;
            if !path.is_empty() && output.send(path.to_owned()).await.is_err() {
                return;
            }
        }
    })
}
