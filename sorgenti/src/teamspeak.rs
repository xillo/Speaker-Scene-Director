use crate::config::Config;
use crate::core::DirectorCore;
use crate::obs::ObsSession;
use serde_json::{json, Value};
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::Sender, Arc};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{client, Error as WsError, Message, WebSocket};

#[derive(Debug)]
pub enum Event {
    Log(String),
    Status(String),
    Scene(String),
}

fn emit(sender: &Sender<Event>, msg: impl Into<String>) {
    let _ = sender.send(Event::Log(msg.into()));
}

fn connect_teamspeak(cfg: &Config) -> Result<WebSocket<TcpStream>, String> {
    let address = format!("{}:{}", cfg.teamspeak.host, cfg.teamspeak.port);
    let endpoint = address.to_socket_addrs().map_err(|e| format!("Resolve TeamSpeak host: {e}"))?
        .next().ok_or("TeamSpeak address not found")?;
    let stream = TcpStream::connect_timeout(&endpoint, Duration::from_secs(3))
        .map_err(|e| format!("Connect to TeamSpeak: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    client(format!("ws://{address}/").as_str(), stream)
        .map(|(socket, _)| socket)
        .map_err(|e| format!("TeamSpeak handshake: {e}"))
}

fn send_auth(socket: &mut WebSocket<TcpStream>, key: &str) -> Result<(), String> {
    socket.send(Message::Text(json!({"type":"auth","payload":{
        "identifier":"org.speaker-scene-director.desktop",
        "version":"1.0.0",
        "name":"Speaker Scene Director",
        "description":"Automatically switch OBS scenes based on TeamSpeak voice activity",
        "content":{"apiKey":key}
    }}).to_string().into())).map_err(|e| format!("TeamSpeak authorization request: {e}"))
}

fn retry_delay(stop: &AtomicBool) {
    for _ in 0..30 {
        if stop.load(Ordering::Relaxed) { return; }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn run(cfg: Config, base_dir: PathBuf, stop: Arc<AtomicBool>, sender: Sender<Event>) {
    let key_file = base_dir.join(".teamspeak_api_key");
    let mut core = DirectorCore::new(cfg.clone(), Instant::now());
    let mut obs: Option<ObsSession> = None;
    let mut obs_retry = Instant::now();
    let mut ignore_saved_key = false;
    emit(&sender, "Director started. Connect to TeamSpeak 6 and OBS Studio.");
    while !stop.load(Ordering::Relaxed) {
        let mut ts = match connect_teamspeak(&cfg) {
            Ok(s) => s,
            Err(e) => {
                let _ = sender.send(Event::Status("Waiting for TeamSpeak".into()));
                emit(&sender, format!("{e}. Retrying in 3 seconds."));
                retry_delay(&stop); continue;
            }
        };
        core.on_reconnect(Instant::now());
        let api_key = if ignore_saved_key { String::new() } else { fs::read_to_string(&key_file).unwrap_or_default().trim().to_string() };
        if let Err(e) = send_auth(&mut ts, &api_key) {
            emit(&sender, e); retry_delay(&stop); continue;
        }
        emit(&sender, "Connected to TeamSpeak. Approve 'Speaker Scene Director' under Remote Apps if prompted.");
        let _ = sender.send(Event::Status("TeamSpeak connected; authorization pending".into()));
        let mut auth_complete = false;
        loop {
            if stop.load(Ordering::Relaxed) { break; }
            match ts.read() {
                Ok(Message::Text(text)) => {
                    let event: Value = match serde_json::from_str(text.as_ref()) {
                        Ok(value) => value,
                        Err(e) => { emit(&sender, format!("Invalid TeamSpeak event: {e}")); continue; }
                    };
                    if event["type"].as_str() == Some("auth") {
                        let code = event["status"]["code"].as_i64().unwrap_or(0);
                        if code != 0 {
                            emit(&sender, format!("TeamSpeak authorization rejected (code {code})."));
                            if !api_key.is_empty() {
                                ignore_saved_key = true;
                                emit(&sender, "Saved token was rejected; requesting authorization again without it.");
                            }
                            break;
                        }
                        if let Some(key) = event["payload"]["apiKey"].as_str() {
                            if !key.is_empty() {
                                if let Err(e) = fs::write(&key_file, key) {
                                    emit(&sender, format!("Unable to save TeamSpeak authorization: {e}"));
                                }
                                ignore_saved_key = false;
                            }
                        }
                        auth_complete = true;
                        let _ = sender.send(Event::Status("TeamSpeak authorized".into()));
                    }
                    for msg in core.handle_ts(&event, Instant::now()) { emit(&sender, msg); }
                }
                Ok(Message::Close(_)) => {
                    emit(&sender, "TeamSpeak disconnected."); break;
                }
                Ok(_) => {}
                Err(WsError::Io(ref e)) if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => { emit(&sender, format!("TeamSpeak connection lost: {e}")); break; }
            }
            if auth_complete {
                if let Some(scene) = core.desired_scene(Instant::now()) {
                    if Instant::now() >= obs_retry {
                        if obs.is_none() {
                            match ObsSession::connect(&cfg.obs) {
                                Ok(s) => { obs = Some(s); emit(&sender, "Connected to OBS WebSocket."); }
                                Err(e) => {
                                    emit(&sender, format!("OBS connection error: {e}"));
                                    obs_retry = Instant::now() + Duration::from_secs(3);
                                }
                            }
                        }
                        if let Some(session) = obs.as_mut() {
                            match session.switch_scene(&scene) {
                                Ok(()) => {
                                    core.scene_changed(scene.clone(), Instant::now());
                                    let _ = sender.send(Event::Scene(scene.clone()));
                                    emit(&sender, format!("OBS scene -> {scene}"));
                                }
                                Err(e) => {
                                    emit(&sender, format!("OBS scene switch error: {e}; retrying in 3 seconds"));
                                    obs = None;
                                    obs_retry = Instant::now() + Duration::from_secs(3);
                                }
                            }
                        }
                    }
                }
            }
        }
        let _ = ts.close(None);
        if !stop.load(Ordering::Relaxed) {
            let _ = sender.send(Event::Status("Reconnecting to TeamSpeak".into()));
            retry_delay(&stop);
        }
    }
    let _ = sender.send(Event::Status("Director stopped".into()));
    emit(&sender, "Director stopped.");
}
