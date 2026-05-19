//! Operator radar parameter panel (egui). Coverage is recomputed only after explicit validation.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::cache::TileCache;
use crate::mesh_cache::MeshCache;
use crate::radar::{Radar, RadarMarker, Radars, ALTITUDE_PRESETS, RCS_PRESETS};
use crate::terrain::sample_terrain_m;

const PANEL_ID: &str = "radar_side_panel";

/// Préréglages affichés dans l'UI (alignés sur `Radar::from_kind`).
const RADAR_PRESETS: &[(&str, &str)] = &[
    ("CIVIL_ATCR", "ATC civil (L/S)"),
    ("MIL_AQ", "Acquisition militaire"),
    ("S-400", "S-400 / Grave Stone"),
    ("BIG BIRD", "Big Bird (64N6)"),
    ("S-300P", "S-300P / Flap Lid"),
    ("S-300V", "S-300V / Grill Pan"),
    ("BUK", "BUK / Snow Drift"),
    ("TOR", "Tor (SA-15)"),
    ("PANTSIR", "Pantsir (SA-22)"),
    ("DON-2N", "Don-2N (ABM)"),
];

#[derive(Resource)]
pub struct RadarEditor {
    pub panel_open: bool,
    pub selected_index: usize,
    pub dirty_target: bool,
    pub dirty_station: bool,
    pub status: String,
    pub pending_validate_target: bool,
    pub pending_validate_station: bool,
    pub pending_cancel: bool,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// MSL — dérivé de terrain site + AGL antenne à la validation.
    pub alt_m: f64,
    /// Hauteur antenne au-dessus du sol du site (réglage opérateur).
    pub antenna_agl_m: f64,
    pub frequency_ghz: f64,
    pub power_dbm: f64,
    pub gain_dbi: f64,
    pub sensitivity_dbm: f64,
    pub target_agl: f32,
    pub target_rcs: f64,
    pub enabled: bool,
    pub committed_agl: f32,
    pub committed_rcs: f64,
}

impl Default for RadarEditor {
    fn default() -> Self {
        Self {
            panel_open: false,
            selected_index: 0,
            dirty_target: false,
            dirty_station: false,
            status: String::new(),
            pending_validate_target: false,
            pending_validate_station: false,
            pending_cancel: false,
            name: String::new(),
            lat: 43.0,
            lon: 7.0,
            alt_m: 1000.0,
            antenna_agl_m: 50.0,
            frequency_ghz: 1.3,
            power_dbm: 55.0,
            gain_dbi: 35.0,
            sensitivity_dbm: -113.0,
            target_agl: 0.0,
            target_rcs: 5.0,
            enabled: true,
            committed_agl: 0.0,
            committed_rcs: 5.0,
        }
    }
}

impl RadarEditor {
    pub fn any_dirty(&self) -> bool {
        self.dirty_target || self.dirty_station
    }

    pub fn load_system_params(&mut self, radars: &Radars) {
        self.target_agl = radars.target_altitude_agl;
        self.target_rcs = radars.target_rcs;
        self.committed_agl = radars.target_altitude_agl;
        self.committed_rcs = radars.target_rcs;
    }

    pub fn load_station(&mut self, radar: &Radar, site_terrain_m: Option<f32>) {
        self.name = radar.name.clone();
        self.lat = radar.position.x;
        self.lon = radar.position.y;
        self.alt_m = radar.position.z;
        self.antenna_agl_m = site_terrain_m
            .map(|t| (radar.position.z - t as f64).max(0.0))
            .unwrap_or(radar.position.z);
        self.frequency_ghz = radar.frequency / 1e9;
        self.power_dbm = radar.transmit_power_dbm;
        self.gain_dbi = radar.gain_dbi;
        self.sensitivity_dbm = radar.sensitivity_dbm;
        self.enabled = radar.enabled;
    }

    pub fn reload_from_committed(&mut self, radars: &Radars, index: usize, site_terrain_m: Option<f32>) {
        self.load_system_params(radars);
        if index < radars.stations.len() {
            self.load_station(&radars.stations[index], site_terrain_m);
        }
        self.dirty_target = false;
        self.dirty_station = false;
        self.status.clear();
    }

    pub fn apply_preset_physics(&mut self, kind: &str) {
        let probe = Radar::from_kind("preset", bevy::math::DVec3::ZERO, kind);
        self.frequency_ghz = probe.frequency / 1e9;
        self.power_dbm = probe.transmit_power_dbm;
        self.gain_dbi = probe.gain_dbi;
        self.sensitivity_dbm = probe.sensitivity_dbm;
        self.dirty_station = true;
    }

    fn draft_range_km(&self, radars: &Radars, idx: usize) -> Option<f64> {
        if idx >= radars.stations.len() {
            return None;
        }
        let committed = &radars.stations[idx];
        let draft = Radar {
            name: committed.name.clone(),
            position: bevy::math::DVec3::new(
                committed.position.x,
                committed.position.y,
                self.alt_m,
            ),
            enabled: self.enabled,
            color: committed.color,
            frequency: self.frequency_ghz * 1e9,
            transmit_power_dbm: self.power_dbm,
            gain_dbi: self.gain_dbi,
            sensitivity_dbm: self.sensitivity_dbm,
        };
        Some(draft.calculate_max_range(self.target_rcs) / 1000.0)
    }

    fn committed_range_km(&self, radars: &Radars, idx: usize) -> Option<f64> {
        radars
            .stations
            .get(idx)
            .map(|r| r.calculate_max_range(self.committed_rcs) / 1000.0)
    }
}

pub fn setup_egui_style(mut contexts: EguiContexts) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals.panel_fill = egui::Color32::from_rgba_premultiplied(18, 22, 28, 245);
    ctx.set_style(style);
}

pub fn sync_radar_editor_system(
    mut editor: ResMut<RadarEditor>,
    radars: Res<Radars>,
    cache: Res<TileCache>,
    mut last_synced: Local<Option<(usize, u64)>>,
) {
    let key = (editor.selected_index, radars.coverage_revision);
    if editor.any_dirty() {
        return;
    }
    if *last_synced == Some(key) {
        return;
    }
    if radars.stations.is_empty() {
        return;
    }
    let idx = editor.selected_index.min(radars.stations.len() - 1);
    editor.selected_index = idx;
    let terrain = sample_terrain_m(&cache, editor.lat, editor.lon);
    editor.reload_from_committed(&radars, idx, terrain);
    *last_synced = Some(key);
}

pub fn radar_panel_keyboard_system(
    mut editor: ResMut<RadarEditor>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if keys.just_pressed(KeyCode::KeyU) {
        editor.panel_open = !editor.panel_open;
        return;
    }

    if !editor.panel_open || !editor.any_dirty() {
        return;
    }

    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if ctrl && keys.just_pressed(KeyCode::Enter) {
        if editor.dirty_target {
            editor.pending_validate_target = true;
        }
        if editor.dirty_station {
            editor.pending_validate_station = true;
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        editor.pending_cancel = true;
    }
}

pub fn radar_panel_ui_system(
    mut contexts: EguiContexts,
    mut editor: ResMut<RadarEditor>,
    mut radars: ResMut<Radars>,
    cache: Res<TileCache>,
    mut mesh_cache: ResMut<MeshCache>,
    mut commands: Commands,
    markers: Query<Entity, With<RadarMarker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    if !editor.panel_open {
        egui::Area::new(egui::Id::new("radar_panel_hint"))
            .fixed_pos(egui::pos2(12.0, 12.0))
            .show(ctx, |ui| {
                let hint = if editor.any_dirty() {
                    "⚙  Radar ●"
                } else {
                    "⚙  Radar"
                };
                if ui
                    .button(egui::RichText::new(hint).size(14.0))
                    .on_hover_text("Panneau paramètres [U]")
                    .clicked()
                {
                    editor.panel_open = true;
                }
            });
        return;
    }

    let mut request_station_index: Option<usize> = None;
    let mut apply_target = editor.pending_validate_target;
    let mut apply_station = editor.pending_validate_station;
    editor.pending_validate_target = false;
    editor.pending_validate_station = false;

    egui::SidePanel::left(PANEL_ID)
        .resizable(true)
        .default_width(380.0)
        .width_range(320.0..=500.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Contrôle radar");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Masquer [U]").clicked() {
                        editor.panel_open = false;
                    }
                });
            });

            draw_model_disclaimer(ui);
            ui.add_space(4.0);
            draw_status_banner(ui, &editor, radars.coverage_revision);

            if !editor.status.is_empty() {
                ui.label(egui::RichText::new(&editor.status).italics().small());
            }

            ui.add_space(6.0);
            ui.separator();

            // --- ① Cible système ---
            egui::CollapsingHeader::new("① Cible simulée — tout le réseau")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Altitude et signature de la cible pour toutes les stations.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.add_space(6.0);

                    egui::Grid::new("system_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Altitude AGL");
                            ui.horizontal(|ui| {
                                if drag_f32(ui, &mut editor.target_agl, 1.0, 0) {
                                    editor.dirty_target = true;
                                }
                                ui.weak("m");
                            });
                            ui.label("RCS");
                            ui.horizontal(|ui| {
                                if drag_f64(ui, &mut editor.target_rcs, 0.1, 2) {
                                    editor.dirty_target = true;
                                }
                                ui.weak("m²");
                            });
                        });

                    ui.add_space(4.0);
                    if preset_row_f32(ui, &ALTITUDE_PRESETS, &mut editor.target_agl) {
                        editor.dirty_target = true;
                    }
                    if preset_row_f64(ui, &RCS_PRESETS, &mut editor.target_rcs) {
                        editor.dirty_target = true;
                    }

                    if editor.dirty_target {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 200, 80),
                            format!(
                                "Brouillon : {:.0} m · {:.1} m²  →  actif : {:.0} m · {:.1} m²",
                                editor.target_agl,
                                editor.target_rcs,
                                editor.committed_agl,
                                editor.committed_rcs
                            ),
                        );
                    }

                    ui.add_space(8.0);
                    if section_apply_button(
                        ui,
                        "Appliquer la cible",
                        editor.dirty_target,
                        "Recalcule la couverture pour toutes les stations (AGL/RCS).",
                    ) {
                        apply_target = true;
                    }
                });

            ui.separator();

            if radars.stations.is_empty() {
                ui.colored_label(egui::Color32::YELLOW, "Aucune station chargée.");
                if draw_global_actions(ui, editor.any_dirty()) {
                    editor.pending_cancel = true;
                }
                return;
            }

            let idx = editor.selected_index.min(radars.stations.len() - 1);
            let site_terrain = sample_terrain_m(&cache, editor.lat, editor.lon);
            if let Some(t) = site_terrain {
                editor.alt_m = t as f64 + editor.antenna_agl_m;
            }

            let station_names: Vec<String> = radars.stations.iter().map(|s| s.name.clone()).collect();

            // --- ② Station ---
            egui::CollapsingHeader::new("② Station sélectionnée")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Les réglages ci-dessous ne concernent que « {} ».",
                            station_names[idx]
                        ))
                        .small()
                        .weak(),
                    );
                    ui.add_space(4.0);

                    ui.add_enabled_ui(!editor.any_dirty(), |ui| {
                        egui::ComboBox::from_id_salt("station_select")
                            .selected_text(&station_names[idx])
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for (i, name) in station_names.iter().enumerate() {
                                    if ui.selectable_label(i == idx, name).clicked() {
                                        request_station_index = Some(i);
                                    }
                                }
                            });
                    });

                    if editor.any_dirty() {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 180, 80),
                            "Validez ou annulez avant de changer de station.",
                        );
                    }

                    egui::Frame::group(ui.style())
                        .fill(egui::Color32::from_rgb(28, 32, 40))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&editor.name).strong());
                            ui.monospace(format!("{:.5}° N  ·  {:.5}° E", editor.lat, editor.lon));
                            ui.label(
                                egui::RichText::new("Position fixe (données GeoJSON)")
                                    .small()
                                    .weak(),
                            );
                        });

                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Site & antenne").small().strong());

                    match site_terrain {
                        Some(t) => {
                            ui.monospace(format!("Terrain SRTM : {:.0} m MSL", t));
                            ui.horizontal(|ui| {
                                ui.label("Antenne AGL");
                                if drag_f64(ui, &mut editor.antenna_agl_m, 1.0, 0) {
                                    editor.dirty_station = true;
                                    editor.alt_m = t as f64 + editor.antenna_agl_m;
                                }
                                ui.weak("m");
                            });
                            ui.weak(format!("→ antenne {:.0} m MSL (brouillon)", editor.alt_m));
                        }
                        None => {
                            ui.colored_label(
                                egui::Color32::YELLOW,
                                "Tuile terrain non chargée — chargez la zone avant validation.",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Antenne MSL");
                                if drag_f64(ui, &mut editor.alt_m, 1.0, 0) {
                                    editor.dirty_station = true;
                                }
                                ui.weak("m");
                            });
                        }
                    }

                    if ui
                        .checkbox(&mut editor.enabled, "Inclure dans le calcul")
                        .changed()
                    {
                        editor.dirty_station = true;
                    }
                });

            // --- ③ Physique ---
            egui::CollapsingHeader::new("③ Physique émetteur (station)")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Préréglage");
                        egui::ComboBox::from_id_salt("radar_kind")
                            .selected_text("Choisir…")
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (kind, label) in RADAR_PRESETS {
                                    if ui
                                        .selectable_label(false, format!("{label} ({kind})"))
                                        .clicked()
                                    {
                                        editor.apply_preset_physics(kind);
                                    }
                                }
                            });
                    });

                    egui::Grid::new("physics_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Fréquence").on_hover_text("Bande radar");
                            ui.horizontal(|ui| {
                                if drag_f64(ui, &mut editor.frequency_ghz, 0.01, 3) {
                                    editor.dirty_station = true;
                                }
                                ui.weak("GHz");
                            });
                            ui.label("Puissance Tx");
                            ui.horizontal(|ui| {
                                if drag_f64(ui, &mut editor.power_dbm, 0.5, 1) {
                                    editor.dirty_station = true;
                                }
                                ui.weak("dBm");
                            });
                            ui.label("Gain");
                            ui.horizontal(|ui| {
                                if drag_f64(ui, &mut editor.gain_dbi, 0.5, 1) {
                                    editor.dirty_station = true;
                                }
                                ui.weak("dBi");
                            });
                            ui.label("Sensibilité");
                            ui.horizontal(|ui| {
                                if drag_f64(ui, &mut editor.sensitivity_dbm, 0.5, 1) {
                                    editor.dirty_station = true;
                                }
                                ui.weak("dBm");
                            });
                        });

                    ui.add_space(8.0);
                    if section_apply_button(
                        ui,
                        "Appliquer la station",
                        editor.dirty_station,
                        &format!(
                            "Recalcule la couverture pour « {} » uniquement (+ paramètres cible déjà actifs).",
                            station_names[idx]
                        ),
                    ) {
                        apply_station = true;
                    }
                });

            ui.separator();
            draw_range_summary(ui, &editor, &radars, idx);

            ui.add_space(8.0);
            ui.weak("Carte 3D : occultation terrain + horizon · pas SNR/clutter");
            ui.weak("Rayons terrain ~500–1000 m");

            ui.add_space(8.0);
            if draw_global_actions(ui, editor.any_dirty()) {
                editor.pending_cancel = true;
            }
        });

    if let Some(new_idx) = request_station_index {
        if !editor.any_dirty() {
            editor.selected_index = new_idx;
            let terrain = sample_terrain_m(&cache, radars.stations[new_idx].position.x, radars.stations[new_idx].position.y);
            editor.load_station(&radars.stations[new_idx], terrain);
        }
    }

    if editor.pending_cancel {
        editor.pending_cancel = false;
        let si = editor.selected_index.min(radars.stations.len().saturating_sub(1));
        let terrain = sample_terrain_m(&cache, editor.lat, editor.lon);
        editor.reload_from_committed(&radars, si, terrain);
        editor.status = "Toutes les modifications annulées.".to_string();
        return;
    }

    if apply_target {
        apply_validated_target(&mut editor, &mut radars, &mut mesh_cache);
    }

    if apply_station {
        apply_validated_station(
            &mut editor,
            &mut radars,
            &cache,
            &mut mesh_cache,
            &mut commands,
            &markers,
            &mut meshes,
            &mut materials,
        );
    }
}

fn draw_model_disclaimer(ui: &mut egui::Ui) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(25, 30, 38))
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Modèle simplifié (vu / pas vu)").small().strong());
            ui.label(
                egui::RichText::new(
                    "Pas de SNR, clutter ni Pd. La carte peut être plus restrictive que la portée théorique.",
                )
                .small()
                .weak(),
            );
        });
}

fn draw_status_banner(ui: &mut egui::Ui, editor: &RadarEditor, revision: u64) {
    let (fill, label, color) = if editor.dirty_target && editor.dirty_station {
        (
            egui::Color32::from_rgb(55, 35, 20),
            "Brouillon : cible + station",
            egui::Color32::from_rgb(255, 160, 60),
        )
    } else if editor.dirty_target {
        (
            egui::Color32::from_rgb(45, 40, 18),
            "Brouillon : cible (réseau)",
            egui::Color32::from_rgb(255, 200, 80),
        )
    } else if editor.dirty_station {
        (
            egui::Color32::from_rgb(40, 30, 50),
            "Brouillon : station seule",
            egui::Color32::from_rgb(200, 160, 255),
        )
    } else {
        (
            egui::Color32::from_rgb(20, 45, 32),
            "Couverture synchronisée",
            egui::Color32::from_rgb(90, 220, 130),
        )
    };

    egui::Frame::group(ui.style())
        .fill(fill)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(color, format!("● {label}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("rev. {revision}"));
                });
            });
        });
}

fn draw_range_summary(ui: &mut egui::Ui, editor: &RadarEditor, radars: &Radars, idx: usize) {
    ui.label(
        egui::RichText::new("Portée radar théorique (espace libre)")
            .small()
            .strong(),
    );
    ui.label(
        egui::RichText::new("≠ zone verte sur la carte (relief, horizon, réseau).")
            .small()
            .weak(),
    );
    egui::Frame::group(ui.style())
        .inner_margin(8.0)
        .show(ui, |ui| {
            if let Some(draft_km) = editor.draft_range_km(radars, idx) {
                ui.label(format!(
                    "Brouillon (σ = {:.1} m²) : {:.1} km",
                    editor.target_rcs, draft_km
                ));
            }
            if let Some(committed_km) = editor.committed_range_km(radars, idx) {
                ui.weak(format!(
                    "Actif (σ = {:.1} m²) : {:.1} km",
                    editor.committed_rcs, committed_km
                ));
            }
        });
}

fn section_apply_button(ui: &mut egui::Ui, label: &str, enabled: bool, tooltip: &str) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(label)
            .fill(if enabled {
                egui::Color32::from_rgb(35, 95, 65)
            } else {
                egui::Color32::from_gray(45)
            }),
    )
    .on_hover_text(tooltip)
    .clicked()
}

fn draw_global_actions(ui: &mut egui::Ui, any_dirty: bool) -> bool {
    ui.separator();
    let clicked = ui
        .horizontal(|ui| {
            ui.add_enabled(any_dirty, egui::Button::new("Tout annuler"))
                .on_hover_text("Réinitialise cible et station [Échap]")
    })
    .inner
    .clicked();
    ui.label(
        egui::RichText::new("Ctrl+Entrée : applique chaque section en brouillon")
            .small()
            .weak(),
    );
    clicked
}

fn drag_f32(ui: &mut egui::Ui, value: &mut f32, speed: f64, decimals: usize) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .speed(speed)
            .max_decimals(decimals),
    )
    .changed()
}

fn drag_f64(ui: &mut egui::Ui, value: &mut f64, speed: f64, decimals: usize) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .speed(speed)
            .max_decimals(decimals),
    )
    .changed()
}

fn preset_row_f32(ui: &mut egui::Ui, presets: &[f32], value: &mut f32) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for &p in presets {
            let selected = (*value - p).abs() < f32::EPSILON;
            let text = if p >= 1000.0 {
                format!("{:.0}k", p / 1000.0)
            } else {
                format!("{:.0}", p)
            };
            if ui.selectable_label(selected, text).clicked() {
                *value = p;
                changed = true;
            }
        }
    });
    changed
}

fn preset_row_f64(ui: &mut egui::Ui, presets: &[f64], value: &mut f64) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for &p in presets {
            let selected = (*value - p).abs() < f64::EPSILON;
            if ui.selectable_label(selected, format!("{:.1}", p)).clicked() {
                *value = p;
                changed = true;
            }
        }
    });
    changed
}

fn bump_coverage(
    editor: &mut RadarEditor,
    radars: &mut Radars,
    mesh_cache: &mut MeshCache,
    message: &str,
) -> u64 {
    radars.coverage_revision = radars.coverage_revision.saturating_add(1);
    let rev = radars.coverage_revision;
    mesh_cache.clear();
    editor.status = format!("{message} (révision {rev}) — recalcul…");
    rev
}

fn apply_validated_target(
    editor: &mut RadarEditor,
    radars: &mut Radars,
    mesh_cache: &mut MeshCache,
) {
    radars.target_altitude_agl = editor.target_agl;
    radars.target_rcs = editor.target_rcs;
    editor.committed_agl = editor.target_agl;
    editor.committed_rcs = editor.target_rcs;
    editor.dirty_target = false;

    let rev = bump_coverage(
        editor,
        radars,
        mesh_cache,
        "Cible appliquée (tout le réseau)",
    );
    info!("Operator validated target AGL={:.0}m RCS={:.1} — revision {}", editor.target_agl, editor.target_rcs, rev);
}

fn apply_validated_station(
    editor: &mut RadarEditor,
    radars: &mut Radars,
    cache: &TileCache,
    mesh_cache: &mut MeshCache,
    commands: &mut Commands,
    markers: &Query<Entity, With<RadarMarker>>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    if radars.stations.is_empty() {
        editor.status = "Erreur : aucune station.".to_string();
        return;
    }

    let idx = editor.selected_index.min(radars.stations.len() - 1);
    let name = radars.stations[idx].name.clone();

    let msl = if let Some(t) = sample_terrain_m(cache, editor.lat, editor.lon) {
        t as f64 + editor.antenna_agl_m
    } else {
        editor.alt_m
    };

    {
        let station = &mut radars.stations[idx];
        station.position.z = msl;
        station.frequency = editor.frequency_ghz * 1e9;
        station.transmit_power_dbm = editor.power_dbm;
        station.gain_dbi = editor.gain_dbi;
        station.sensitivity_dbm = editor.sensitivity_dbm;
        station.enabled = editor.enabled;
    }

    editor.alt_m = msl;
    editor.dirty_station = false;

    let rev = bump_coverage(
        editor,
        radars,
        mesh_cache,
        &format!("Station « {name} » appliquée"),
    );

    for entity in markers.iter() {
        commands.entity(entity).despawn();
    }
    crate::radar::spawn_radar_markers(commands, meshes, materials, radars);

    info!("Operator validated station '{}' — revision {}", name, rev);
}
