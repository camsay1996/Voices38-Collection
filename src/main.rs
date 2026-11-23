use eframe::egui;
use rayon::prelude::*;
use std::time::Duration;
use std::sync::mpsc::{channel, Receiver};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;

// ============================================================
// DATA STRUCTURES
// ============================================================
#[derive(Serialize, Deserialize, Clone)]
struct Link {
    label: String,
    url: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Game {
    name: String,
    links: Vec<Link>,
}

#[derive(Clone)]
struct LinkStatus {
    text: String,
    color: egui::Color32,
}

struct Voices38App {
    search_query: String,
    filter: String,
    statuses: std::collections::HashMap<String, LinkStatus>,
    is_checking: bool,
    rx: Option<Receiver<(String, LinkStatus)>>,
    
    // Dynamic Data
    games: Vec<Game>,
    total_links: usize,
    load_error: Option<String>,
    
    // Auto-Updater State
    update_status: Option<String>,
}

impl Default for Voices38App {
    fn default() -> Self {
        let mut games: Vec<Game> = Vec::new();
        let mut load_error = None;

        match fs::read_to_string("games.json") {
            Ok(json_str) => {
                match serde_json::from_str(&json_str) {
                    Ok(parsed_games) => games = parsed_games,
                    Err(e) => load_error = Some(format!("Failed to parse games.json: {}", e)),
                }
            }
            Err(e) => load_error = Some(format!("Could not find games.json in app folder.\nError: {}", e)),
        }

        let total_links = games.iter().flat_map(|g| &g.links).count();

        Self {
            search_query: String::new(),
            filter: "All".to_owned(),
            statuses: std::collections::HashMap::new(),
            is_checking: false,
            rx: None,
            games,
            total_links,
            load_error,
            update_status: None,
        }
    }
}

// ============================================================
// LINK CHECKER
// ============================================================
fn check_url(url: &str) -> LinkStatus {
    use std::net::ToSocketAddrs;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .resolver(|addr: &str| {
            // addr will be something like "buzzheavier.com:443"
            if addr.starts_with("buzzheavier.com") {
                // Return the hardcoded Cloudflare IP with the correct port
                let port = addr.rsplit(':').next().unwrap_or("443");
                let port: u16 = port.parse().unwrap_or(443);
                Ok(vec![std::net::SocketAddr::from(([104, 18, 4, 146], port))])
            } else {
                // Normal DNS for everything else
                addr.to_socket_addrs().map(|iter| iter.collect())
            }
        })
        .build();

    match agent.head(url).call() {
        Ok(res) if res.status() < 400 => LinkStatus { text: "ALIVE".into(), color: egui::Color32::from_rgb(166, 227, 161) },
        Ok(res) if res.status() == 403 => LinkStatus { text: "BLOCKED".into(), color: egui::Color32::from_rgb(249, 226, 175) },
        Ok(res) if res.status() == 404 => LinkStatus { text: "DEAD".into(), color: egui::Color32::from_rgb(243, 139, 168) },
        Ok(res) => LinkStatus { text: format!("HTTP {}", res.status()), color: egui::Color32::from_rgb(249, 226, 175) },
        Err(_) => LinkStatus { text: "DEAD".into(), color: egui::Color32::from_rgb(243, 139, 168) },
    }
}

// ============================================================
// UI IMPLEMENTATION
// ============================================================
impl eframe::App for Voices38App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        // 1. MODERN THEME SETUP
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(30, 30, 46);
        visuals.window_fill = egui::Color32::from_rgb(30, 30, 46);
        visuals.extreme_bg_color = egui::Color32::from_rgb(24, 24, 37);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(49, 50, 68);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(69, 71, 90);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(137, 180, 250);
        visuals.selection.bg_fill = egui::Color32::from_rgb(137, 180, 250);
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
        ctx.set_style(style);

        // 2. ERROR SCREEN
        if let Some(err) = &self.load_error {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(err).color(egui::Color32::RED).size(16.0));
                });
            });
            return;
        }

        // 3. THREAD LISTENERS
        if let Some(rx) = &self.rx {
            while let Ok((url, status)) = rx.try_recv() {
                self.statuses.insert(url, status);
            }
            if self.statuses.len() == self.total_links {
                self.is_checking = false;
                self.rx = None;
            }
        }

        // 4. POPUP WINDOW FOR UPDATER
        let mut close_popup = false;
        let popup_msg = self.update_status.clone();

        if let Some(msg) = popup_msg {
            egui::Window::new("Updater")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(msg);
                    if ui.button("OK").clicked() {
                        close_popup = true;
                    }
                });
        }

        if close_popup {
            self.update_status = None;
        }

        // 5. TOP PANEL
        egui::TopBottomPanel::top("top_panel")
            .frame(egui::Frame::default().inner_margin(15.0).fill(egui::Color32::from_rgb(24, 24, 37)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(egui::RichText::new("🎮 Voices38 Collection").color(egui::Color32::from_rgb(137, 180, 250)).strong());
                    ui.add_space(20.0);
                    
                    ui.label("🔍");
                    ui.add_sized([250.0, 25.0], egui::TextEdit::singleline(&mut self.search_query).hint_text("Search games..."));
                    
                    ui.add_space(10.0);
                    
                    egui::ComboBox::from_id_salt("filter")
                        .selected_text(&self.filter)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.filter, "All".to_owned(), "All Games");
                            ui.selectable_value(&mut self.filter, "Full only".to_owned(), "Full only");
                            ui.selectable_value(&mut self.filter, "Has Crack".to_owned(), "Has Crack");
                            ui.selectable_value(&mut self.filter, "Has Mirror".to_owned(), "Has Mirror");
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🗑 Clear").clicked() {
                            self.statuses.clear();
                        }
                        
                        if self.is_checking {
                            ui.spinner();
                            ui.label(egui::RichText::new("Checking...").color(egui::Color32::from_rgb(249, 226, 175)));
                        } else {
                            if ui.button("🔗 Check Links").clicked() {
                                self.is_checking = true;
                                self.statuses.clear();
                                let (tx, rx) = channel();
                                self.rx = Some(rx);
                                let all_urls: Vec<String> = self.games.iter().flat_map(|g| g.links.iter().map(|l| l.url.clone())).collect();
                                let ctx_clone = ctx.clone();
                                
                                std::thread::spawn(move || {
                                    all_urls.par_iter().for_each(|url| {
                                        let status = check_url(url);
                                        let _ = tx.send((url.clone(), status));
                                        ctx_clone.request_repaint();
                                    });
                                });
                            }
                        }

                        // --- AUTO UPDATER BUTTON ---
                        if ui.button("⬇ Update List").clicked() {
                            self.update_status = None;
                            
                            // Get current timestamp to bypass GitHub's raw file cache
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs();

                            // Use the LATEST URL (removed the commit hash) + add cache buster
                            let url = format!(
                                "https://gist.githubusercontent.com/camsay1996/7bdb1b8ce11ec7aad68fca8c86c5c6ba/raw/games.json?t={}",
                                timestamp
                            );

                            // Make the request
                            let res = ureq::get(&url).call();
                            
                            match res {
                                Ok(response) => {
                                    let mut json_str = String::new();
                                    if response.into_reader().read_to_string(&mut json_str).is_ok() {
                                        if let Ok(new_games) = serde_json::from_str::<Vec<Game>>(&json_str) {
                                            if fs::write("games.json", &json_str).is_ok() {
                                                self.games = new_games;
                                                self.total_links = self.games.iter().flat_map(|g| &g.links).count();
                                                self.statuses.clear();
                                                self.load_error = None;
                                                
                                                // Show success message
                                                self.update_status = Some("✅ Successfully updated games list!".to_string());
                                                ctx.request_repaint(); // <-- Forces the UI to refresh immediately
                                            } else {
                                                self.update_status = Some("❌ Failed to save games.json to disk.".to_string());
                                            }
                                        } else {
                                            self.update_status = Some("❌ Downloaded file is not valid JSON.".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.update_status = Some(format!("❌ Failed to download update:\n{}", e));
                                }
                            }
                        }
                    });
                });
            });

        // 6. BOTTOM STATUS BAR
        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(egui::Frame::default().inner_margin(10.0).fill(egui::Color32::from_rgb(24, 24, 37)))
            .show(ctx, |ui| {
                let alive = self.statuses.values().filter(|s| s.color == egui::Color32::from_rgb(166, 227, 161)).count();
                let dead = self.statuses.values().filter(|s| s.color == egui::Color32::from_rgb(243, 139, 168)).count();
                let blocked = self.statuses.len() - alive - dead;
                
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("✅ Alive: {}", alive)).color(egui::Color32::from_rgb(166, 227, 161)));
                    ui.label(egui::RichText::new(format!("❌ Dead: {}", dead)).color(egui::Color32::from_rgb(243, 139, 168)));
                    ui.label(egui::RichText::new(format!("⚠️ Blocked: {}", blocked)).color(egui::Color32::from_rgb(249, 226, 175)));
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("Total Links Checked: {}/{}", self.statuses.len(), self.total_links));
                        if self.is_checking {
                            let progress = self.statuses.len() as f32 / self.total_links as f32;
                            ui.add(egui::ProgressBar::new(progress).show_percentage().desired_width(150.0));
                        }
                    });
                });
            });

        // 7. MAIN CENTRAL AREA
        egui::CentralPanel::default()
            .frame(egui::Frame::default().inner_margin(15.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for game in &self.games {
                        if !self.search_query.is_empty() && !game.name.to_lowercase().contains(&self.search_query.to_lowercase()) {
                            continue;
                        }
                        if self.filter == "Full only" && !game.links.iter().any(|l| l.label.to_lowercase() == "full") { continue; }
                        if self.filter == "Has Crack" && !game.links.iter().any(|l| l.label.to_lowercase().contains("crack")) { continue; }
                        if self.filter == "Has Mirror" && !game.links.iter().any(|l| l.label.to_lowercase() == "mirror") { continue; }

                        let card_frame = egui::Frame::none()
                            .fill(egui::Color32::from_rgb(49, 50, 68))
                            .rounding(egui::Rounding::same(10.0))
                            .inner_margin(15.0)
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(69, 71, 90)));

                        card_frame.show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&game.name).strong().size(17.0).color(egui::Color32::from_rgb(205, 214, 244)));
                                ui.add_space(5.0);
                                
                                ui.horizontal_wrapped(|ui| {
                                    for link in &game.links {
                                        let btn = egui::Button::new(
                                            egui::RichText::new(&link.label).size(13.0).color(egui::Color32::from_rgb(137, 180, 250))
                                        )
                                        .fill(egui::Color32::from_rgb(30, 30, 46))
                                        .rounding(egui::Rounding::same(6.0));
                                        
                                        if ui.add(btn).clicked() {
                                            ui.ctx().open_url(egui::OpenUrl::new_tab(&link.url));
                                        }
                                        
                                        if let Some(status) = self.statuses.get(&link.url) {
                                            let badge = egui::RichText::new(&status.text).color(status.color).size(11.0).strong();
                                            ui.label(badge);
                                        } else {
                                            ui.label(egui::RichText::new("not checked").color(egui::Color32::from_rgb(108, 112, 134)).size(11.0));
                                        }
                                        ui.add_space(10.0);
                                    }
                                });
                            });
                        });
                        ui.add_space(10.0);
                    }
                });
            });
    }
}

// ============================================================
// MAIN ENTRY POINT
// ============================================================
fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1050.0, 750.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Voices38 Definitive Collection",
        options,
        Box::new(|_cc| Ok(Box::new(Voices38App::default()))),
    )
}