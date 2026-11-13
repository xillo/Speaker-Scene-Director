use crate::config::ObsSettings;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use tungstenite::{client, Message, WebSocket};

pub struct ObsSession {
    ws: WebSocket<TcpStream>,
    next_request: u64,
}

fn read_json(ws: &mut WebSocket<TcpStream>) -> Result<Value, String> {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                return serde_json::from_str(text.as_ref()).map_err(|e| format!("Invalid OBS JSON: {e}"));
            }
            Ok(Message::Close(_)) => return Err("OBS closed the WebSocket connection".into()),
            Ok(_) => continue,
            Err(e) => return Err(format!("OBS WebSocket read: {e}")),
        }
    }
}

fn write_json(ws: &mut WebSocket<TcpStream>, obj: &Value) -> Result<(), String> {
    ws.send(Message::Text(obj.to_string().into())).map_err(|e| format!("OBS WebSocket write: {e}"))
}

pub fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    let first = Sha256::digest(format!("{password}{salt}").as_bytes());
    let secret = STANDARD.encode(first);
    STANDARD.encode(Sha256::digest(format!("{secret}{challenge}").as_bytes()))
}

impl ObsSession {
    pub fn connect(settings: &ObsSettings) -> Result<Self, String> {
        let address = format!("{}:{}", settings.host, settings.port);
        let socket_addr = address.to_socket_addrs()
            .map_err(|e| format!("Resolve OBS host: {e}"))?.next()
            .ok_or_else(|| "OBS address not found".to_string())?;
        let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(3))
            .map_err(|e| format!("Connect to OBS at {address}: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
        let (mut ws, _) = client(format!("ws://{address}/").as_str(), stream)
            .map_err(|e| format!("OBS WebSocket handshake: {e}"))?;
        let hello = read_json(&mut ws)?;
        if hello["op"].as_i64() != Some(0) {
            return Err("OBS did not send a Hello (protocol 5 required)".into());
        }
        let mut identify = json!({"op": 1, "d": {"rpcVersion": 1, "eventSubscriptions": 0}});
        if let Some(auth) = hello["d"]["authentication"].as_object() {
            let salt = auth.get("salt").and_then(Value::as_str).ok_or("OBS Hello missing salt")?;
            let challenge = auth.get("challenge").and_then(Value::as_str).ok_or("OBS Hello missing challenge")?;
            identify["d"]["authentication"] = json!(authentication(&settings.password, salt, challenge));
        }
        write_json(&mut ws, &identify)?;
        let identified = read_json(&mut ws)?;
        if identified["op"].as_i64() != Some(2) {
            return Err(format!("OBS authentication/identification rejected: {}", identified["op"]));
        }
        Ok(Self { ws, next_request: 1 })
    }

    fn request(&mut self, kind: &str, data: Value) -> Result<Value, String> {
        let request_id = format!("speaker-director-{}", self.next_request);
        self.next_request += 1;
        write_json(&mut self.ws, &json!({
            "op": 6,
            "d": {"requestType": kind, "requestId": request_id, "requestData": data}
        }))?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let response = read_json(&mut self.ws)?;
            if response["op"].as_i64() != Some(7) || response["d"]["requestId"].as_str() != Some(request_id.as_str()) {
                continue;
            }
            if response["d"]["requestStatus"]["result"].as_bool() != Some(true) {
                let code = response["d"]["requestStatus"]["code"].as_i64().unwrap_or(-1);
                let message = response["d"]["requestStatus"]["comment"].as_str().unwrap_or("Unknown OBS error");
                return Err(format!("OBS {kind} failed ({code}): {message}"));
            }
            return Ok(response["d"]["responseData"].clone());
        }
        Err(format!("OBS {kind} timed out"))
    }

    pub fn scene_names(&mut self) -> Result<Vec<String>, String> {
        let response = self.request("GetSceneList", json!({}))?;
        Ok(response["scenes"].as_array().ok_or("OBS did not return scenes")?
            .iter().filter_map(|item| item["sceneName"].as_str().map(str::to_owned)).collect())
    }

    pub fn switch_scene(&mut self, scene: &str) -> Result<(), String> {
        self.request("SetCurrentProgramScene", json!({"sceneName": scene}))?;
        Ok(())
    }
}

pub fn test_scenes(settings: &ObsSettings, expected: &[String]) -> Result<Vec<String>, String> {
    let mut obs = ObsSession::connect(settings)?;
    let scenes = obs.scene_names()?;
    let missing: Vec<_> = expected.iter().filter(|scene| !scenes.contains(scene)).cloned().collect();
    if !missing.is_empty() {
        return Err(format!("Missing OBS scenes: {}. Available: {}", missing.join(", "), scenes.join(", ")));
    }
    Ok(scenes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_string_matches_example_algorithm() {
        let password = "pass";
        let salt = "salt";
        let challenge = "challenge";
        let secret = STANDARD.encode(Sha256::digest(b"passsalt"));
        let expected = STANDARD.encode(Sha256::digest(format!("{secret}challenge").as_bytes()));
        assert_eq!(authentication(password, salt, challenge), expected);
    }
}
