// SPDX-License-Identifier: AGPL-3.0-or-later
//! Chánh Điện's existing control protocol. One connection per application,
//! independent of the page lifetime. Failed writes are never replayed.
use serde_json::{Value, json};
use std::{
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};
use tungstenite::{Message, protocol::WebSocketConfig};
use url::Url;

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub origin: String,
    pub overlay: String,
    pub studio: String,
    pub websocket: String,
    url: Url,
}

impl Endpoint {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let mut url = Url::parse(value).map_err(|_| "Địa chỉ máy chủ LIVE không hợp lệ")?;
        let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if !matches!(url.scheme(), "http" | "https")
            || (!local && url.scheme() != "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("Dùng địa chỉ gốc HTTPS hoặc HTTP localhost của máy chủ LIVE");
        }
        let origin = url.origin().ascii_serialization();
        let overlay = format!("{origin}/live/overlay.html");
        let studio = format!("{origin}/live/studio.html");
        url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
            .ok();
        url.set_path("/ws");
        url.set_query(Some("role=control"));
        Ok(Self {
            origin,
            overlay,
            studio,
            websocket: url.to_string(),
            url,
        })
    }
}

/// Canonical protocol from the original server.mjs and control.html.
pub fn command(action: &str, name: &str, text: &str, n: i32) -> Option<Value> {
    let payload = match action {
        "connect" if !name.trim().is_empty() => json!({"cmd":"connect", "uniqueId":name.trim()}),
        "disconnect" => json!({"cmd":"disconnect"}),
        "chat" if !text.trim().is_empty() => json!({"cmd":"chat", "name":name, "text":text}),
        "like" => json!({"cmd":"like", "name":name}),
        "gift" => json!({"cmd":"gift", "name":name, "gift":text, "diamonds":n.max(1), "count":1}),
        "fill" if [15, 36, 64, 100].contains(&n) => json!({"cmd":"fill", "n":n}),
        "bots" => json!({"cmd":"bots", "on":n > 0, "n":18, "pace":"mid"}),
        "clear" => json!({"cmd":"clear"}),
        "mo" | "bell" | "chant" | "stop" => json!({"cmd":action}),
        "say" if !text.trim().is_empty() => json!({"cmd":"say", "text":text}),
        "skin" if ["monk", "child", "nun", "granny", "man", "woman"].contains(&text) => {
            json!({"cmd":"skin", "skin":text})
        }
        "pin" => json!({"cmd":"pin", "title":name, "price":text}),
        _ => return None,
    };
    let mut payload = payload;
    payload["t"] = json!("ctl");
    Some(payload)
}

#[derive(Debug)]
pub enum Event {
    Connected,
    Disconnected,
    Message(Value),
    WriteFailed,
}

/// The callback returns false when its UI owner is gone. Reconnect only
/// restores subscriptions; it does not reconnect TikTok or repeat commands.
pub fn run(
    endpoint: Endpoint,
    commands: Receiver<Value>,
    stop: Arc<AtomicBool>,
    mut publish: impl FnMut(Event) -> bool,
) {
    let mut delay = 1_u64;
    while !stop.load(Ordering::Relaxed) {
        let connection = (|| {
            let addresses = endpoint
                .url
                .socket_addrs(|| {
                    Some(if endpoint.url.scheme() == "wss" {
                        443
                    } else {
                        80
                    })
                })
                .map_err(|_| ())?;
            let mut stream = None;
            for address in addresses {
                if let Ok(socket) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
                    stream = Some(socket);
                    break;
                }
            }
            let stream = stream.ok_or(())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .map_err(|_| ())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(|_| ())?;
            let config = WebSocketConfig::default()
                .max_message_size(Some(512 * 1024))
                .max_frame_size(Some(512 * 1024));
            let (mut socket, _) = tungstenite::client_tls_with_config(
                endpoint.websocket.as_str(),
                stream,
                Some(config),
                None,
            )
            .map_err(|_| ())?;
            match socket.get_mut() {
                tungstenite::stream::MaybeTlsStream::Plain(s) => {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(80)));
                }
                tungstenite::stream::MaybeTlsStream::NativeTls(s) => {
                    let _ = s
                        .get_mut()
                        .set_read_timeout(Some(Duration::from_millis(80)));
                }
                _ => {}
            }
            Ok::<_, ()>(socket)
        })();
        if let Ok(mut socket) = connection {
            let connected_at = Instant::now();
            if !publish(Event::Connected) {
                return;
            }
            'connected: while !stop.load(Ordering::Relaxed) {
                // Bound each batch so incoming state/logs cannot starve.
                for _ in 0..16 {
                    match commands.try_recv() {
                        Ok(value) => {
                            if socket
                                .send(Message::Text(value.to_string().into()))
                                .is_err()
                            {
                                if !publish(Event::WriteFailed) {
                                    return;
                                }
                                break 'connected;
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            let _ = socket.close(None);
                            return;
                        }
                    }
                }
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        if let Ok(value) = serde_json::from_str(&text) {
                            if !publish(Event::Message(value)) {
                                return;
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                    _ => {}
                }
            }
            if connected_at.elapsed() >= Duration::from_secs(5) {
                delay = 1;
            }
            let _ = socket.close(None);
        }
        if stop.load(Ordering::Relaxed) || !publish(Event::Disconnected) {
            return;
        }
        let until = Instant::now() + Duration::from_secs(delay);
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            // Drop queued writes during downtime; never replay a user action.
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(_) => {
                    if !publish(Event::WriteFailed) {
                        return;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(_) => {}
            }
        }
        delay = (delay * 2).min(30);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actions_match_the_existing_wire_protocol() {
        assert_eq!(
            command("connect", " @minhan ", "", 0),
            Some(json!({"t":"ctl","cmd":"connect","uniqueId":"@minhan"}))
        );
        assert_eq!(
            command("gift", "An", "Đèn hoa sen", 30),
            Some(
                json!({"t":"ctl","cmd":"gift","name":"An","gift":"Đèn hoa sen","diamonds":30,"count":1})
            )
        );
        assert_eq!(command("fill", "", "", 7), None);
        assert_eq!(command("chat", "An", "", 0), None);
        assert_eq!(command("skin", "", "../../", 0), None);
    }
    #[test]
    fn every_route_uses_the_configured_origin() {
        let endpoint = Endpoint::parse("http://localhost:4199").unwrap();
        assert_eq!(endpoint.overlay, "http://localhost:4199/live/overlay.html");
        assert_eq!(endpoint.websocket, "ws://localhost:4199/ws?role=control");
        assert!(Endpoint::parse("http://example.com").is_err());
        assert!(Endpoint::parse("http://user:secret@localhost:3088").is_err());
        assert!(Endpoint::parse("https://example.com/path").is_err());
    }

    #[test]
    fn a_real_socket_delivers_state_and_only_the_explicit_command() {
        use std::{net::TcpListener, sync::mpsc, thread};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint =
            Endpoint::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let (commands, receive) = mpsc::channel();
        let (events, observed) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            run(endpoint, receive, worker_stop, move |event| {
                events.send(event).is_ok()
            })
        });
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        let mut server = tungstenite::accept(socket).unwrap();
        assert!(matches!(
            observed.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Connected
        ));
        // Subscribing must not start TikTok, a bot, TTS or a live broadcast.
        assert!(matches!(server.read(), Err(tungstenite::Error::Io(_))));
        server
            .send(Message::Text(
                json!({"t":"state","viewers":[],"cap":24})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        assert!(
            matches!(observed.recv_timeout(Duration::from_secs(3)).unwrap(), Event::Message(value) if value["cap"] == 24)
        );
        commands
            .send(command("chat", "Test", "Cầu an", 0).unwrap())
            .unwrap();
        server
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let received = server.read().unwrap().into_text().unwrap();
        let payload: Value = serde_json::from_str(&received).unwrap();
        assert_eq!(
            payload,
            json!({"t":"ctl","cmd":"chat","name":"Test","text":"Cầu an"})
        );
        stop.store(true, Ordering::Relaxed);
        drop(server);
        worker.join().unwrap();
    }
}
