//! Scene-switching state machine, intentionally independent from network or UI.
use crate::config::Config;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

type ClientKey = (String, String); // TeamSpeak (connectionId, clientId)

#[derive(Default, Clone, Debug)]
struct Client {
    nickname: String,
    uid: String,
    channel: Option<String>,
}

#[derive(Clone, Debug)]
struct Voice {
    started: Instant,
    ended_at: Option<Instant>,
}

pub struct DirectorCore {
    cfg: Config,
    clients: HashMap<ClientKey, Client>,
    self_ids: HashMap<String, String>,
    connection_id: Option<String>,
    speaking: HashMap<ClientKey, Voice>,
    pub current_scene: Option<String>,
    last_scene_change: Option<Instant>,
    last_voice_time: Instant,
    last_candidate: Option<String>,
    candidate_since: Instant,
}

fn field_string(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() { return Some(s.to_string()); }
    if let Some(n) = value.as_i64() { return Some(n.to_string()); }
    if let Some(n) = value.as_u64() { return Some(n.to_string()); }
    None
}

impl DirectorCore {
    pub fn new(cfg: Config, now: Instant) -> Self {
        Self {
            cfg,
            clients: HashMap::new(),
            self_ids: HashMap::new(),
            connection_id: None,
            speaking: HashMap::new(),
            current_scene: None,
            last_scene_change: None,
            last_voice_time: now,
            last_candidate: None,
            candidate_since: now,
        }
    }

    pub fn on_reconnect(&mut self, now: Instant) {
        self.clients.clear();
        self.self_ids.clear();
        self.connection_id = None;
        self.speaking.clear();
        self.last_candidate = None;
        self.last_voice_time = now;
    }

    fn parse_client(&mut self, connection_id: &str, info: &Value) {
        let client_id = field_string(&info["id"]).or_else(|| field_string(&info["clientId"]));
        let Some(client_id) = client_id else { return; };
        let record = self.clients.entry((connection_id.to_string(), client_id)).or_default();
        let props = &info["properties"];
        if let Some(n) = field_string(&props["nickname"]).or_else(|| field_string(&info["nickname"])) {
            record.nickname = n;
        }
        if let Some(uid) = field_string(&props["uniqueIdentifier"]).or_else(|| field_string(&info["uniqueIdentifier"])) {
            record.uid = uid;
        }
        if let Some(channel) = field_string(&info["channelId"]) {
            record.channel = Some(channel);
        }
    }

    fn find_scene(&self, key: &ClientKey) -> Option<String> {
        let person = self.clients.get(key)?;
        if self.cfg.same_channel_only {
            if let Some(self_id) = self.self_ids.get(&key.0) {
                if let Some(us) = self.clients.get(&(key.0.clone(), self_id.clone())) {
                    if let (Some(a), Some(b)) = (&person.channel, &us.channel) {
                        if a != b { return None; }
                    }
                }
            }
        }
        for p in &self.cfg.participants {
            if let Some(ref configured_uid) = p.unique_identifier {
                if !configured_uid.is_empty() && *configured_uid == person.uid {
                    return Some(p.scene.clone());
                }
            } else if !person.nickname.trim().is_empty() &&
                p.nickname.trim().to_lowercase() == person.nickname.trim().to_lowercase() {
                return Some(p.scene.clone());
            }
        }
        None
    }

    pub fn handle_ts(&mut self, event: &Value, now: Instant) -> Vec<String> {
        let mut logs = Vec::new();
        let event_type = event["type"].as_str().unwrap_or("");
        let payload = &event["payload"];
        match event_type {
            "auth" => {
                self.connection_id = field_string(&payload["currentConnectionId"]);
                if let Some(conns) = payload["connections"].as_array() {
                    for conn in conns {
                        if let Some(id) = field_string(&conn["id"]) {
                            if let Some(self_id) = field_string(&conn["clientId"]) {
                                self.self_ids.insert(id.clone(), self_id);
                            }
                            if let Some(infos) = conn["clientInfos"].as_array() {
                                for info in infos { self.parse_client(&id, info); }
                            }
                        }
                    }
                }
                let mut names: Vec<_> = self.clients.values().map(|c| c.nickname.as_str()).filter(|n| !n.is_empty()).collect();
                names.sort_unstable(); names.dedup();
                logs.push(format!("TeamSpeak authorized. Participants: {}", names.join(", ")));
            }
            "clientPropertiesUpdated" | "clientSelfPropertyUpdated" => {
                let conn = field_string(&payload["connectionId"]).or_else(|| self.connection_id.clone());
                if let Some(conn) = conn {
                    let client = field_string(&payload["clientId"])
                        .or_else(|| if event_type == "clientSelfPropertyUpdated" { self.self_ids.get(&conn).cloned() } else { None });
                    if let Some(client) = client {
                        self.parse_client(&conn, &serde_json::json!({"id":client, "properties": payload["properties"]}));
                    }
                }
            }
            "clientMoved" | "clientMove" => {
                let conn = field_string(&payload["connectionId"]).or_else(|| self.connection_id.clone());
                let client = field_string(&payload["clientId"]);
                if let (Some(conn), Some(client)) = (conn, client) {
                    let channel = field_string(&payload["channelId"])
                        .or_else(|| field_string(&payload["targetChannelId"]));
                    if let Some(channel) = channel {
                        self.parse_client(&conn, &serde_json::json!({"id":client, "channelId":channel}));
                    }
                }
            }
            "talkStatusChanged" => {
                // A TS6 event may belong to a different tab than currentConnectionId.
                // Match by the event's own connectionId and clientId, as in the working Python v3.
                let conn = field_string(&payload["connectionId"]).or_else(|| self.connection_id.clone());
                let cid = field_string(&payload["clientId"]);
                if let (Some(conn), Some(cid)) = (conn, cid) {
                    let key = (conn.clone(), cid.clone());
                    let status = payload["status"].as_i64().unwrap_or(0);
                    if status == 1 {
                        let grace = Duration::from_millis(self.cfg.release_grace_ms);
                        if let Some(voice) = self.speaking.get_mut(&key) {
                            if voice.ended_at.map(|end| now.saturating_duration_since(end) <= grace).unwrap_or(true) {
                                voice.ended_at = None;
                            } else {
                                *voice = Voice { started: now, ended_at: None };
                            }
                        } else {
                            self.speaking.insert(key.clone(), Voice { started: now, ended_at: None });
                        }
                        let name = self.clients.get(&key).map(|c| c.nickname.as_str()).unwrap_or("Unknown client");
                        if let Some(scene) = self.find_scene(&key) {
                            self.last_voice_time = now;
                            logs.push(format!("Speaking: {name} -> {scene} (connection {conn})"));
                        } else if self.cfg.log_events {
                            logs.push(format!("Speaking: {name}, no matching scene (connection {conn})"));
                        }
                    } else if status == 0 || status == 2 {
                        if let Some(voice) = self.speaking.get_mut(&key) {
                            voice.ended_at = if status == 2 {
                                Some(now.checked_sub(Duration::from_millis(self.cfg.release_grace_ms + 1)).unwrap_or(now))
                            } else { Some(now) };
                        }
                    }
                } else if self.cfg.log_events {
                    logs.push("Received incomplete TeamSpeak voice event".into());
                }
            }
            "connectStatusChanged" => {
                self.speaking.clear();
                self.last_candidate = None;
                if self.cfg.log_events { logs.push("TeamSpeak connection status changed".into()); }
            }
            _ => {
                if self.cfg.log_events { logs.push(format!("TeamSpeak event: {event_type}")); }
            }
        }
        logs
    }

    pub fn desired_scene(&mut self, now: Instant) -> Option<String> {
        let grace = Duration::from_millis(self.cfg.release_grace_ms);
        self.speaking.retain(|_, voice| voice.ended_at
            .map(|end| now.saturating_duration_since(end) <= grace).unwrap_or(true));
        let mut active: Vec<_> = self.speaking.iter()
            .filter_map(|(key, voice)| self.find_scene(key).map(|scene| (voice.started, scene)))
            .collect();
        if !active.is_empty() {
            self.last_voice_time = now;
            if let Some(current) = &self.current_scene {
                if active.iter().any(|(_, scene)| scene == current) {
                    self.last_candidate = None;
                    return None;
                }
            }
            active.sort_by_key(|(started, _)| *started);
            let scene = active.last().expect("non-empty").1.clone();
            if self.last_candidate.as_ref() != Some(&scene) {
                self.last_candidate = Some(scene.clone());
                self.candidate_since = now;
            }
            if now.saturating_duration_since(self.candidate_since) >= Duration::from_millis(self.cfg.switch_delay_ms)
                && self.minimum_scene_elapsed(now) && self.current_scene.as_ref() != Some(&scene) {
                return Some(scene);
            }
        } else {
            self.last_candidate = None;
            if now.saturating_duration_since(self.last_voice_time) >= Duration::from_millis(self.cfg.silence_timeout_ms)
                && self.minimum_scene_elapsed(now) && self.current_scene.as_ref() != Some(&self.cfg.default_scene) {
                return Some(self.cfg.default_scene.clone());
            }
        }
        None
    }

    fn minimum_scene_elapsed(&self, now: Instant) -> bool {
        self.last_scene_change.map(|t| now.saturating_duration_since(t) >= Duration::from_millis(self.cfg.min_scene_ms)).unwrap_or(true)
    }

    pub fn scene_changed(&mut self, scene: String, now: Instant) {
        self.current_scene = Some(scene);
        self.last_scene_change = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn config() -> Config {
        let mut c = Config::default();
        c.participants = vec![
            crate::config::Participant { nickname: "babyxillo".into(), scene: "Davide".into(), unique_identifier: None },
            crate::config::Participant { nickname: "m4nvit".into(), scene: "Emanuele".into(), unique_identifier: None },
        ];
        c.default_scene = "Davide".into();
        c
    }
    #[test]
    fn different_connection_id_still_switches() {
        let t = Instant::now();
        let mut d = DirectorCore::new(config(), t);
        d.handle_ts(&json!({"type":"auth","payload":{"currentConnectionId":1,
            "connections":[{"id":2,"clientId":106,"clientInfos":[
                {"id":106,"properties":{"nickname":"babyxillo"}},
                {"id":101,"properties":{"nickname":"m4nvit"}}
            ]}]}}), t);
        d.handle_ts(&json!({"type":"talkStatusChanged","payload":{"connectionId":2,"clientId":101,"status":1}}), t);
        assert_eq!(d.desired_scene(t), None);
        assert_eq!(d.desired_scene(t + Duration::from_millis(200)), None);
        assert_eq!(d.desired_scene(t + Duration::from_millis(501)).as_deref(), Some("Emanuele"));
        d.scene_changed("Emanuele".into(), t + Duration::from_millis(501));
        d.handle_ts(&json!({"type":"talkStatusChanged","payload":{"connectionId":2,"clientId":101,"status":0}}), t + Duration::from_secs(1));
        assert_eq!(d.desired_scene(t + Duration::from_secs(2)), None);
        assert_eq!(d.desired_scene(t + Duration::from_secs(7)).as_deref(), Some("Davide"));
    }
    #[test]
    fn short_pausess_are_joined() {
        let t = Instant::now();
        let mut d = DirectorCore::new(config(), t);
        d.handle_ts(&json!({"type":"auth","payload":{"connections":[{"id":2,"clientId":106,"clientInfos":[{"id":101,"properties":{"nickname":"m4nvit"}}]}]}}), t);
        d.handle_ts(&json!({"type":"talkStatusChanged","payload":{"connectionId":2,"clientId":101,"status":1}}), t);
        assert_eq!(d.desired_scene(t + Duration::from_millis(100)), None);
        d.handle_ts(&json!({"type":"talkStatusChanged","payload":{"connectionId":2,"clientId":101,"status":0}}), t + Duration::from_millis(250));
        d.handle_ts(&json!({"type":"talkStatusChanged","payload":{"connectionId":2,"clientId":101,"status":1}}), t + Duration::from_millis(600));
        assert_eq!(d.desired_scene(t + Duration::from_millis(650)).as_deref(), Some("Emanuele"));
    }
}
