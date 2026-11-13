use crate::config::{self, Config, Participant};
use crate::obs;
use crate::teamspeak::{self, Event};
use eframe::egui;
use rand::{distributions::Alphanumeric, Rng};
use std::path::PathBuf;
use std::collections::BTreeMap;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, Sender}, Arc};
use std::thread::{self, JoinHandle};

#[derive(Clone, Copy, PartialEq)]
enum Tab { Director, Participants, Settings, VideoLinks }

struct Worker {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

pub struct DirectorApp {
    cfg: Config,
    dir: PathBuf,
    tab: Tab,
    worker: Option<Worker>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    lines: Vec<String>,
    status: String,
    current_scene: String,
    video_id: String,
    video_person: String,
    video_ids: BTreeMap<String, String>,
}

impl DirectorApp {
    pub fn new(cfg: Config, dir: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        let video_ids: BTreeMap<String, String> = std::fs::read_to_string(dir.join("vdo_links.json"))
            .ok().and_then(|contents| serde_json::from_str(&contents).ok()).unwrap_or_default();
        let video_person = cfg.participants.first().map(|p| p.nickname.clone()).unwrap_or_default();
        let video_id = video_ids.get(&video_person).cloned().unwrap_or_default();
        Self {
            current_scene: "Not selected".into(),
            cfg, dir, tab: Tab::Director, worker: None,
            sender, receiver, lines: vec!["Ready. Connect to OBS Studio and TeamSpeak 6.".into()],
            status: "Stopped".into(), video_id, video_person, video_ids,
        }
    }

    fn add_log(&mut self, line: impl Into<String>) {
        let text = line.into();
        self.lines.push(text);
        if self.lines.len() > 400 { self.lines.drain(..self.lines.len() - 400); }
    }

    fn running(&self) -> bool { self.worker.is_some() }

    fn start(&mut self) {
        if self.worker.is_some() { return; }
        if let Err(e) = self.cfg.validate() { self.add_log(format!("Configuration error: {e}")); return; }
        if let Err(e) = self.save() { self.add_log(format!("Unable to save settings: {e}")); return; }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let sender = self.sender.clone();
        let cfg = self.cfg.clone();
        let dir = self.dir.clone();
        let handle = thread::spawn(move || teamspeak::run(cfg, dir, flag, sender));
        self.worker = Some(Worker { stop, handle });
        self.status = "Starting".into();
        self.add_log("Director starting...");
    }

    fn stop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.stop.store(true, Ordering::Relaxed);
            self.status = "Stopping".into();
            self.add_log("Stop requested. Waiting for network operations to finish...");
        }
    }

    fn save(&mut self) -> Result<(), String> {
        let result = config::save(&self.dir.join("config.json"), &self.cfg);
        if result.is_ok() { self.add_log("Settings saved to config.json."); }
        result
    }

    fn test_obs(&mut self) {
        let cfg = self.cfg.clone();
        let sender = self.sender.clone();
        self.add_log("Testing OBS WebSocket and scene names...");
        thread::spawn(move || {
            let mut expected = vec![cfg.default_scene];
            for p in &cfg.participants { if !expected.contains(&p.scene) { expected.push(p.scene.clone()); } }
            match obs::test_scenes(&cfg.obs, &expected) {
                Ok(scenes) => { let _ = sender.send(Event::Log(format!("OBS test passed. Available scenes: {}", scenes.join(", ")))); }
                Err(e) => { let _ = sender.send(Event::Log(format!("OBS test failed: {e}"))); }
            }
        });
    }

    fn top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("Speaker Scene Director");
                ui.separator();
                ui.label("TeamSpeak 6 -> OBS Studio");
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (name, tab) in [
                    ("Director", Tab::Director),
                    ("Participants", Tab::Participants),
                    ("Settings", Tab::Settings),
                    ("VDO.Ninja links", Tab::VideoLinks),
                ] {
                    if ui.selectable_label(self.tab == tab, name).clicked() { self.tab = tab; }
                }
            });
            ui.add_space(6.0);
        });
    }

    fn director_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Automatic scene switching");
        ui.label("The person talking in TeamSpeak determines the active OBS scene.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.running(), egui::Button::new("Start director")).clicked() { self.start(); }
            if ui.add_enabled(self.running(), egui::Button::new("Stop director")).clicked() { self.stop(); }
            if ui.button("Test OBS").clicked() { self.test_obs(); }
            if ui.button("Save settings").clicked() {
                if let Err(e) = self.save() { self.add_log(format!("Save failed: {e}")); }
            }
        });
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.strong("Status:"); ui.label(&self.status);
            ui.separator();
            ui.strong("Current scene:"); ui.label(&self.current_scene);
            ui.separator();
            ui.strong("Default:"); ui.label(&self.cfg.default_scene);
        });
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Activity log");
        ui.horizontal(|ui| {
            ui.label("Voice events are received locally. Authorization keys are never printed.");
            if ui.button("Clear log").clicked() { self.lines.clear(); }
        });
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for line in &self.lines { ui.label(line); }
        });
    }

    fn participants_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("TeamSpeak participants and OBS scenes");
        ui.label("Nicknames must match TeamSpeak. Scene names must match OBS exactly.");
        ui.add_space(8.0);
        let mut remove_index = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, participant) in self.cfg.participants.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(format!("Participant {}", index + 1));
                        if ui.small_button("Remove").clicked() { remove_index = Some(index); }
                    });
                    ui.horizontal(|ui| {
                        ui.label("TeamSpeak nickname");
                        ui.text_edit_singleline(&mut participant.nickname);
                        ui.label("OBS scene");
                        ui.text_edit_singleline(&mut participant.scene);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Unique identifier (optional)");
                        ui.text_edit_singleline(participant.unique_identifier.get_or_insert_with(String::new));
                        if participant.unique_identifier.as_deref() == Some("") { participant.unique_identifier = None; }
                    });
                });
            }
        });
        if let Some(index) = remove_index { self.cfg.participants.remove(index); }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Add participant").clicked() { self.cfg.participants.push(Participant::default()); }
            if ui.button("Save participants").clicked() {
                if let Err(e) = self.save() { self.add_log(format!("Save failed: {e}")); }
            }
        });
        if self.running() { ui.colored_label(egui::Color32::YELLOW, "Stop and restart the director to apply changes."); }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connections and switching rules");
        ui.add_space(5.0);
        egui::Grid::new("connection_grid").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.strong("TeamSpeak 6 host"); ui.text_edit_singleline(&mut self.cfg.teamspeak.host); ui.end_row();
            ui.strong("TeamSpeak 6 port"); ui.add(egui::DragValue::new(&mut self.cfg.teamspeak.port).range(1..=65535)); ui.end_row();
            ui.strong("OBS WebSocket host"); ui.text_edit_singleline(&mut self.cfg.obs.host); ui.end_row();
            ui.strong("OBS WebSocket port"); ui.add(egui::DragValue::new(&mut self.cfg.obs.port).range(1..=65535)); ui.end_row();
            ui.strong("OBS password"); ui.add(egui::TextEdit::singleline(&mut self.cfg.obs.password).password(true).desired_width(250.0)); ui.end_row();
            ui.strong("Default OBS scene"); ui.text_edit_singleline(&mut self.cfg.default_scene); ui.end_row();
        });
        ui.add_space(12.0);
        ui.separator();
        ui.heading("Automatic switching delays");
        egui::Grid::new("timing_grid").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.label("Switch delay (ms)"); ui.add(egui::DragValue::new(&mut self.cfg.switch_delay_ms).range(0..=30_000)); ui.end_row();
            ui.label("Minimum scene duration (ms)"); ui.add(egui::DragValue::new(&mut self.cfg.min_scene_ms).range(0..=120_000)); ui.end_row();
            ui.label("Silence timeout (ms)"); ui.add(egui::DragValue::new(&mut self.cfg.silence_timeout_ms).range(0..=120_000)); ui.end_row();
            ui.label("Voice release grace (ms)"); ui.add(egui::DragValue::new(&mut self.cfg.release_grace_ms).range(0..=30_000)); ui.end_row();
        });
        ui.add_space(8.0);
        ui.checkbox(&mut self.cfg.same_channel_only, "Same TeamSpeak channel only");
        ui.checkbox(&mut self.cfg.log_events, "Verbose TeamSpeak event log");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save settings").clicked() {
                if let Err(e) = self.save() { self.add_log(format!("Save failed: {e}")); }
            }
            if ui.button("Test OBS connection").clicked() { self.test_obs(); }
        });
        ui.label("Configuration is stored locally beside the application in config.json.");
        if self.running() { ui.colored_label(egui::Color32::YELLOW, "Restart the director to apply changed settings."); }
    }

    fn save_video_link(&mut self) -> Result<(), String> {
        if self.video_person.trim().is_empty() { return Err("Select a participant first".into()); }
        if self.video_id.is_empty() || !self.video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err("Invalid stream ID".into());
        }
        self.video_ids.insert(self.video_person.clone(), self.video_id.clone());
        let path = self.dir.join("vdo_links.json");
        let text = serde_json::to_string_pretty(&self.video_ids).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }

    fn links_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("VDO.Ninja: receive an OBS Virtual Camera");
        ui.label("Your friend selects OBS Virtual Camera as the camera and disables microphone audio.");
        ui.label("Add the viewer link as a Browser Source in your OBS scene.");
        ui.add_space(8.0);
        let previously_selected = self.video_person.clone();
        let participant_names: Vec<String> = self.cfg.participants.iter().map(|p| p.nickname.clone()).collect();
        egui::ComboBox::from_label("Participant")
            .selected_text(if self.video_person.is_empty() { "Select a participant" } else { self.video_person.as_str() })
            .show_ui(ui, |ui| {
                for nickname in participant_names {
                    ui.selectable_value(&mut self.video_person, nickname.clone(), nickname);
                }
            });
        if self.video_person != previously_selected {
            self.video_id = self.video_ids.get(&self.video_person).cloned().unwrap_or_default();
        }
        ui.horizontal(|ui| {
            ui.label("Stream ID");
            ui.text_edit_singleline(&mut self.video_id);
            if ui.button("Generate random ID").clicked() {
                self.video_id = rand::rngs::OsRng.sample_iter(&Alphanumeric).take(24).map(char::from).collect();
                if let Err(e) = self.save_video_link() { self.add_log(format!("Save VDO.Ninja link failed: {e}")); }
            }
        });
        if ui.button("Save link for participant").clicked() {
            if let Err(e) = self.save_video_link() { self.add_log(format!("Save VDO.Ninja link failed: {e}")); }
        }
        let id_valid = !self.video_id.is_empty() && self.video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !id_valid {
            ui.label("Enter an ID using letters, numbers, hyphen, or underscore, or generate one.");
            return;
        }
        let push = format!("https://vdo.ninja/?push={}", self.video_id);
        let view = format!("https://vdo.ninja/?view={}", self.video_id);
        ui.separator();
        ui.label("Friend's publishing link (open on friend's computer)");
        ui.horizontal_wrapped(|ui| {
            ui.monospace(&push);
            if ui.button("Copy publishing link").clicked() { ui.ctx().copy_text(push.clone()); }
        });
        ui.add_space(8.0);
        ui.label("OBS viewer link (add as Browser Source)");
        ui.horizontal_wrapped(|ui| {
            ui.monospace(&view);
            if ui.button("Copy viewer link").clicked() { ui.ctx().copy_text(view.clone()); }
        });
        ui.add_space(12.0);
        ui.colored_label(egui::Color32::YELLOW, "Anyone who learns a link may be able to access the stream. Do not share links publicly.");
        ui.label("VDO.Ninja transports video; this application only switches OBS scenes.");
    }
}

impl eframe::App for DirectorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Log(line) => self.add_log(line),
                Event::Status(status) => self.status = status,
                Event::Scene(scene) => self.current_scene = scene,
            }
        }
        if self.worker.as_ref().map(|w| w.handle.is_finished()).unwrap_or(false) {
            if let Some(worker) = self.worker.take() { let _ = worker.handle.join(); }
            self.status = "Stopped".into();
        }
        self.top(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.tab {
                Tab::Director => self.director_tab(ui),
                Tab::Participants => self.participants_tab(ui),
                Tab::Settings => self.settings_tab(ui),
                Tab::VideoLinks => self.links_tab(ui),
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

impl Drop for DirectorApp {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() { worker.stop.store(true, Ordering::Relaxed); }
    }
}
