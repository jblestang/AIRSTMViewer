//! Radial terrain cross-section from the selected radar toward the mouse cursor (500 km).

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use egui_plot::{Line, LineStyle, Plot, PlotPoints, Points, Polygon, VLine};

use crate::cache::TileCache;
use crate::radar::{Radars, VisibilityCause};
use crate::radar_panel::RadarEditor;
use crate::terrain::{
    destination_point, haversine_m, initial_bearing_deg, sample_terrain_nearest_m,
    sample_terrain_nearest_on_tile, CursorTerrain,
};
use crate::tile::{TileCoord, TileData, TileState};

pub const MAX_PROFILE_RANGE_KM: f64 = 500.0;
/// March along the radial at SRTM1 spacing (~1 arc-second).
const DEM_FINE_STEP_M: f64 = 30.0;
/// Display bin width; each point = max(nearest DEM samples) over this span.
const PROFILE_BIN_M: f64 = 250.0;
const R_EFF: f64 = 6_371_000.0 * (4.0 / 3.0);
const PROFILE_PANEL_MIN_HEIGHT: f32 = 200.0;
const PROFILE_PANEL_DEFAULT_HEIGHT: f32 = 280.0;
/// Fixed MSL altitude range — Y scale does not change with zoom or profile data.
const Y_AXIS_MIN_M: f64 = 0.0;
const Y_AXIS_MAX_M: f64 = 2_000.0;

#[derive(Resource)]
pub struct RadialProfileUi {
    pub panel_open: bool,
    /// Last bearing used when the cursor is not on terrain.
    pub last_bearing_deg: f64,
}

impl Default for RadialProfileUi {
    fn default() -> Self {
        Self {
            panel_open: true,
            last_bearing_deg: 0.0,
        }
    }
}

#[derive(Clone)]
struct ProfileSample {
    dist_km: f64,
    terrain_m: f64,
}

pub fn update_cursor_terrain_system(
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    cache: Res<TileCache>,
    mut cursor: ResMut<CursorTerrain>,
    _profile_ui: ResMut<RadialProfileUi>,
) {
    let (camera, camera_transform) = *camera;
    let mut hit = None;

    if let Some(cursor_position) = window.cursor_position() {
        if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) {
            hit = crate::terrain::pick_terrain_under_cursor(ray.origin, ray.direction, &cache);
        }
    }

    if let Some((lat, lon, terrain_m)) = hit {
        cursor.lat = Some(lat);
        cursor.lon = Some(lon);
        cursor.terrain_m = Some(terrain_m);
    } else {
        cursor.lat = None;
        cursor.lon = None;
        cursor.terrain_m = None;
    }
}

pub fn radial_profile_keyboard_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut profile_ui: ResMut<RadialProfileUi>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        profile_ui.panel_open = !profile_ui.panel_open;
    }
}

pub fn radial_profile_ui_system(
    mut contexts: EguiContexts,
    cache: Res<TileCache>,
    radars: Res<Radars>,
    editor: Res<RadarEditor>,
    cursor: Res<CursorTerrain>,
    mut profile_ui: ResMut<RadialProfileUi>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    if !profile_ui.panel_open {
        return;
    }

    egui::TopBottomPanel::top("radial_profile_panel")
        .resizable(true)
        .default_height(PROFILE_PANEL_DEFAULT_HEIGHT)
        .height_range(PROFILE_PANEL_MIN_HEIGHT..=480.0)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;

            ui.horizontal(|ui| {
                ui.heading("Coupe radiale");
                ui.weak(format!("0 — {MAX_PROFILE_RANGE_KM:.0} km"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Masquer [V]").clicked() {
                        profile_ui.panel_open = false;
                    }
                });
            });

            if radars.stations.is_empty() {
                ui.label("Aucune station radar — chargez des sites ou activez le réseau.");
                return;
            }

            let idx = editor.selected_index.min(radars.stations.len() - 1);
            let radar = &radars.stations[idx];
            let radar_lat = radar.position.x;
            let radar_lon = radar.position.y;
            let radar_msl = radar.position.z;

            let agl = if editor.dirty_target {
                editor.target_agl
            } else {
                radars.target_altitude_agl
            };
            let rcs = if editor.dirty_target {
                editor.target_rcs
            } else {
                radars.target_rcs
            };

            let bearing = if let (Some(clat), Some(clon)) = (cursor.lat, cursor.lon) {
                let b = initial_bearing_deg(radar_lat, radar_lon, clat, clon);
                profile_ui.last_bearing_deg = b;
                b
            } else {
                profile_ui.last_bearing_deg
            };

            let profile = build_radial_profile(&cache, radar_lat, radar_lon, bearing);

            let snapshot = cache.get_snapshot();

            let target_info = match (cursor.lat, cursor.lon) {
                (Some(clat), Some(clon)) => {
                    let dist_m = haversine_m(radar_lat, radar_lon, clat, clon);
                    let dist_km = dist_m / 1000.0;
                    let ground = cursor.terrain_m.unwrap_or(0.0) as f64;
                    let target_msl = ground + agl as f64;
                    let los = radar.classify_visibility(
                        clat,
                        clon,
                        target_msl as f32,
                        rcs,
                        &snapshot,
                    );
                    let mask_km = find_terrain_mask_km(
                        &profile,
                        dist_m,
                        radar_msl,
                        target_msl,
                    );
                    Some(TargetOnProfile {
                        dist_km,
                        target_msl,
                        ground_m: ground,
                        los,
                        mask_km,
                    })
                }
                _ => None,
            };

            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Origine : {}", radar.name));
                ui.separator();
                ui.weak(format!("Cap {:.1}°", bearing));
                ui.separator();
                ui.label(format!("Cible : AGL {agl:.0} m · RCS {rcs:.1} m²"));
                if let Some(t) = &target_info {
                    ui.separator();
                    if t.dist_km <= MAX_PROFILE_RANGE_KM {
                        ui.weak(format!(
                            "Curseur {:.1} km · sol {:.0} m → {:.0} m MSL",
                            t.dist_km, t.ground_m, t.target_msl
                        ));
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 180, 80),
                            format!("Curseur {:.1} km (hors échelle)", t.dist_km),
                        );
                    }
                    ui.separator();
                    draw_los_status(ui, t);
                } else {
                    ui.separator();
                    ui.weak("Curseur hors terrain");
                }
            });

            if profile.is_empty() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Relief indisponible — chargez les tuiles DEM le long du rayon.",
                );
                return;
            }

            let y_floor = profile
                .iter()
                .map(|s| s.terrain_m)
                .fold(f64::INFINITY, f64::min)
                .min(0.0)
                - 200.0;

            let terrain_pts: PlotPoints = profile
                .iter()
                .map(|s| [s.dist_km, s.terrain_m])
                .collect();

            let mut fill_pts: Vec<[f64; 2]> = profile.iter().map(|s| [s.dist_km, s.terrain_m]).collect();
            if let Some(last) = profile.last() {
                fill_pts.push([last.dist_km, y_floor]);
            }
            fill_pts.push([0.0, y_floor]);

            let target_for_plot = target_info.clone();
            let plot_width = ui.available_width().max(64.0);
            let plot_height = ui.available_height().max(80.0);

            Plot::new("radial_cross_section")
                .width(plot_width)
                .height(plot_height)
                .x_axis_label("Distance depuis le radar (km)")
                .y_axis_label("Altitude MSL (m)")
                .default_x_bounds(0.0, MAX_PROFILE_RANGE_KM)
                .default_y_bounds(Y_AXIS_MIN_M, Y_AXIS_MAX_M)
                .auto_bounds(egui::Vec2b::new(false, false))
                .show_axes([true, true])
                .show_grid([true, true])
                .allow_zoom(egui::Vec2b::new(true, false))
                .allow_drag(egui::Vec2b::new(true, false))
                .allow_scroll(egui::Vec2b::new(true, false))
                .show(ui, |plot_ui| {
                    plot_ui.polygon(
                        Polygon::new("relief", fill_pts)
                            .fill_color(egui::Color32::from_rgba_premultiplied(60, 90, 50, 80)),
                    );

                    plot_ui.line(
                        Line::new("relief (terrain)", terrain_pts)
                            .color(egui::Color32::from_rgb(120, 200, 90))
                            .width(2.0),
                    );

                    plot_ui.points(
                        Points::new("radar", vec![[0.0, radar_msl]])
                            .color(egui::Color32::from_rgb(80, 220, 255))
                            .radius(6.0),
                    );

                    if let Some(t) = target_for_plot {
                        if t.dist_km <= MAX_PROFILE_RANGE_KM {
                            let ray_pts = los_ray_profile_points(
                                &profile,
                                t.dist_km * 1000.0,
                                radar_msl,
                                t.target_msl,
                            );
                            if !ray_pts.is_empty() {
                                plot_ui.line(
                                    Line::new("trajectoire LOS", ray_pts)
                                        .color(egui::Color32::from_rgba_premultiplied(180, 200, 255, 140))
                                        .width(1.0)
                                        .style(LineStyle::dotted_loose()),
                                );
                            }

                            let (los_color, los_name) = los_line_style(t.los);
                            plot_ui.line(
                                Line::new(los_name, vec![[0.0, radar_msl], [t.dist_km, t.target_msl]])
                                    .color(los_color)
                                    .width(2.0)
                                    .style(LineStyle::dashed_dense()),
                            );

                            if let Some(mask_km) = t.mask_km {
                                let mask_h = los_altitude_m(
                                    mask_km * 1000.0,
                                    t.dist_km * 1000.0,
                                    radar_msl,
                                    t.target_msl,
                                );
                                plot_ui.points(
                                    Points::new(
                                        "masque relief",
                                        vec![[mask_km, mask_h]],
                                    )
                                    .color(egui::Color32::from_rgb(255, 140, 40))
                                    .radius(5.0),
                                );
                                plot_ui.vline(
                                    VLine::new("masque", mask_km)
                                        .color(egui::Color32::from_rgb(255, 140, 40))
                                        .style(LineStyle::dotted_loose()),
                                );
                            }

                            let target_color = if t.los.is_visible() {
                                egui::Color32::from_rgb(80, 255, 120)
                            } else {
                                egui::Color32::from_rgb(255, 90, 90)
                            };
                            plot_ui.points(
                                Points::new("cible", vec![[t.dist_km, t.target_msl]])
                                    .color(target_color)
                                    .radius(7.0),
                            );
                        }
                    }

                    plot_ui.vline(
                        VLine::new("500 km", MAX_PROFILE_RANGE_KM)
                            .color(egui::Color32::from_gray(100))
                            .style(LineStyle::dashed_loose()),
                    );
                });
        });
}

#[derive(Clone)]
struct TargetOnProfile {
    dist_km: f64,
    target_msl: f64,
    ground_m: f64,
    los: VisibilityCause,
    /// First terrain intersection along the profile (visual hint).
    mask_km: Option<f64>,
}

fn draw_los_status(ui: &mut egui::Ui, t: &TargetOnProfile) {
    let (color, detail) = match t.los {
        VisibilityCause::Visible => (
            egui::Color32::from_rgb(80, 220, 120),
            String::new(),
        ),
        VisibilityCause::Disabled => (
            egui::Color32::from_rgb(160, 160, 160),
            String::new(),
        ),
        VisibilityCause::OutOfRange {
            distance_m,
            max_range_m,
        } => (
            egui::Color32::from_rgb(255, 160, 80),
            format!(
                " ({:.1} km > portée {:.1} km)",
                distance_m / 1000.0,
                max_range_m / 1000.0
            ),
        ),
        VisibilityCause::RadioHorizon {
            distance_m,
            horizon_m,
        } => (
            egui::Color32::from_rgb(255, 200, 80),
            format!(
                " ({:.1} km > horizon {:.1} km)",
                distance_m / 1000.0,
                horizon_m / 1000.0
            ),
        ),
        VisibilityCause::TerrainOccluded => {
            let mask = t
                .mask_km
                .map(|k| format!(" — relief ~{k:.1} km"))
                .unwrap_or_default();
            (
                egui::Color32::from_rgb(255, 90, 90),
                mask,
            )
        }
    };

    ui.horizontal(|ui| {
        ui.label("LOS :");
        ui.colored_label(
            color,
            egui::RichText::new(format!("{}{}", t.los.label_fr(), detail)).strong(),
        );
        if t.los == VisibilityCause::Visible {
            ui.weak("(cohérent avec la cartographie)");
        }
    });
}

fn los_line_style(cause: VisibilityCause) -> (egui::Color32, &'static str) {
    if cause.is_visible() {
        (
            egui::Color32::from_rgb(80, 255, 140),
            "LOS — visible",
        )
    } else {
        (
            egui::Color32::from_rgb(255, 70, 70),
            "LOS — masquée",
        )
    }
}

/// Ray altitude along the radial profile (4/3 Earth curvature, same model as coverage).
fn los_altitude_m(dist_m: f64, total_dist_m: f64, radar_msl: f64, target_msl: f64) -> f64 {
    if total_dist_m < 1.0 {
        return radar_msl;
    }
    let t = (dist_m / total_dist_m).clamp(0.0, 1.0);
    let linear_h = radar_msl + (target_msl - radar_msl) * t;
    let drop = (dist_m * (total_dist_m - dist_m)) / (2.0 * R_EFF);
    linear_h - drop
}

fn los_ray_profile_points(
    profile: &[ProfileSample],
    total_dist_m: f64,
    radar_msl: f64,
    target_msl: f64,
) -> Vec<[f64; 2]> {
    profile
        .iter()
        .filter(|s| s.dist_km * 1000.0 <= total_dist_m + 1.0)
        .map(|s| {
            let d_m = s.dist_km * 1000.0;
            [s.dist_km, los_altitude_m(d_m, total_dist_m, radar_msl, target_msl)]
        })
        .collect()
}

fn find_terrain_mask_km(
    profile: &[ProfileSample],
    total_dist_m: f64,
    radar_msl: f64,
    target_msl: f64,
) -> Option<f64> {
    for s in profile {
        let d_m = s.dist_km * 1000.0;
        if d_m < 50.0 || d_m > total_dist_m {
            continue;
        }
        let ray_h = los_altitude_m(d_m, total_dist_m, radar_msl, target_msl);
        if s.terrain_m > ray_h + 5.0 {
            return Some(s.dist_km);
        }
    }
    None
}

fn build_radial_profile(
    cache: &TileCache,
    origin_lat: f64,
    origin_lon: f64,
    bearing_deg: f64,
) -> Vec<ProfileSample> {
    let max_m = MAX_PROFILE_RANGE_KM * 1000.0;
    let num_bins = (max_m / PROFILE_BIN_M).ceil() as usize + 1;
    let mut bin_max: Vec<f64> = vec![f64::NEG_INFINITY; num_bins];

    let mut d = 0.0;
    let mut current_coord: Option<TileCoord> = None;
    let mut current_data: Option<&TileData> = None;

    while d <= max_m + DEM_FINE_STEP_M * 0.5 {
        let bin_idx = (d / PROFILE_BIN_M).floor() as usize;
        if bin_idx >= num_bins {
            break;
        }

        let (lat, lon) = if d < 0.5 {
            (origin_lat, origin_lon)
        } else {
            destination_point(origin_lat, origin_lon, bearing_deg, d)
        };

        let coord = TileCoord::from_world_coords(lat, lon);
        if current_coord != Some(coord) {
            current_coord = Some(coord);
            current_data = match cache.tiles.get(&coord) {
                Some(TileState::Loaded(data)) => Some(data.as_ref()),
                _ => None,
            };
        }

        let h = if let Some(data) = current_data {
            sample_terrain_nearest_on_tile(data, coord, lat, lon)
        } else {
            sample_terrain_nearest_m(cache, lat, lon)
        };

        if let Some(h) = h {
            let h = h as f64;
            if h > bin_max[bin_idx] {
                bin_max[bin_idx] = h;
            }
        }

        d += DEM_FINE_STEP_M;
    }

    bin_max
        .into_iter()
        .enumerate()
        .map(|(i, terrain_m)| ProfileSample {
            dist_km: (i as f64 * PROFILE_BIN_M) / 1000.0,
            terrain_m: if terrain_m.is_finite() {
                terrain_m
            } else {
                0.0
            },
        })
        .collect()
}
