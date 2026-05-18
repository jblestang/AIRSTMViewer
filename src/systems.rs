// Systems for coordinating tile loading and mesh updates
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures_lite::future;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use crate::cache::TileCache;
use crate::colormap::ColorMap;
use crate::downloader::TileDownloader;
use crate::lod::LodManager;
use crate::mesh_builder::{CachedMeshData, TerrainMeshBuilder};
use crate::mesh_cache::{make_cache_key, MeshCache};
use crate::tile::{TileCoord, TileState};

/// Component marking a terrain tile entity
#[derive(Component)]
pub struct TerrainTile {
    pub coord: TileCoord,
    pub lod: usize,
    pub radar_params: (f32, f64, usize),
}

/// Marker for tiles that need mesh regeneration
#[derive(Component)]
pub struct NeedsRegen;

/// Component for tracking background mesh generation tasks
#[derive(Component)]
pub struct MeshGenTask {
    pub task: Task<Arc<CachedMeshData>>,
    pub coord: TileCoord,
    pub lod: usize,
    pub radar_params: (f32, f64, usize),
}

/// Component for tracking background disk loading tasks
#[derive(Component)]
pub struct TileLoadTask {
    pub task: Task<Result<crate::tile::TileData, String>>,
    pub coord: TileCoord,
}

/// System to determine visible tiles and request loading
pub fn tile_loader_system(
    mut commands: Commands,
    camera_query: Query<&Transform, With<Camera>>,
    mut cache: ResMut<TileCache>,
    downloader: Res<TileDownloader>,
    mut last_cam_pos: Local<Option<Vec3>>,
) {
    let Ok(camera_transform) = camera_query.single() else {
        return;
    };

    let cam_pos = camera_transform.translation;

    // Skip if the camera hasn't moved more than 0.25 tiles since last check.
    // tile_size = 3601 world units → threshold = 900 world units ≈ 0.25 tiles.
    const MOVE_THRESHOLD_SQ: f32 = 900.0 * 900.0;
    if let Some(last) = *last_cam_pos {
        if cam_pos.distance_squared(last) < MOVE_THRESHOLD_SQ {
            return;
        }
    }
    *last_cam_pos = Some(cam_pos);

    // Calculate which tile the camera is over
    let tile_size = 3601.0;
    let lat_idx = (-cam_pos.z / tile_size).ceil() as i32 - 1;
    let center_coord = TileCoord::new(
        lat_idx,
        (cam_pos.x / tile_size).floor() as i32,
    );

    // Radius scales with altitude but is tightly capped.
    // Physical horizon at altitude h (m): sqrt(2 * R_eff * h) ≈ 505 km at 15 km.
    // One degree of latitude ≈ 111 km  →  max visible ≈ 4.5 tiles at 15 km.
    // We add a ×1.5 buffer for radar LOS calculations beyond the visual horizon.
    // Old formula gave radius=18 at y=15 000 (37×37=1 369 tiles); new cap is 7 (15×15=225).
    let tile_radius = (cam_pos.y / 200.0).max(3.0).min(7.0).ceil() as i32;

    for dlat in -tile_radius..=tile_radius {
        for dlon in -tile_radius..=tile_radius {
            let coord = TileCoord::new(center_coord.lat + dlat, center_coord.lon + dlon);

            if cache.has_tile(&coord) {
                continue;
            }

            if cache.is_cached_on_disk(&coord) {
                let thread_pool = AsyncComputeTaskPool::get();
                let path = cache.get_tile_path(&coord);
                let task = thread_pool.spawn(async move {
                    load_tile_from_path(path, coord)
                });
                commands.spawn(TileLoadTask { task, coord });
                cache.mark_loading(coord);
            } else {
                cache.mark_loading(coord);
                downloader.request_download(coord);
            }
        }
    }
}

/// System to evict distant tiles from memory
pub fn cache_eviction_system(
    camera_query: Query<&Transform, With<Camera>>,
    mut cache: ResMut<TileCache>,
    tile_entities: Query<(Entity, &TerrainTile)>,
    mut commands: Commands,
) {
    let camera_transform = camera_query.single();
    let cam_pos = camera_transform.unwrap().translation;
    let tile_size = 3601.0f32;
    let eviction_radius_sq = (tile_size * 100.0).powi(2); // Generous eviction (100 tiles)

    // 1. Evict from memory cache
    let mut to_remove = Vec::new();
    for (coord, state) in cache.tiles.iter() {
        if let TileState::Loaded(_) = state {
            let tile_pos = Vec3::new(coord.lon as f32 * tile_size, 0.0, -(coord.lat as f32 * tile_size));
            if cam_pos.distance_squared(tile_pos) > eviction_radius_sq {
                to_remove.push(*coord);
            }
        }
    }

    for coord in to_remove {
        // info!("Evicting tile from memory: {:?}", coord);
        cache.remove_tile(&coord);
    }

    // 2. Despawn distant entities (optional, but good for performance)
    for (entity, tile) in tile_entities.iter() {
        let tile_pos = Vec3::new(tile.coord.lon as f32 * tile_size, 0.0, -(tile.coord.lat as f32 * tile_size));
        if cam_pos.distance_squared(tile_pos) > eviction_radius_sq {
            if let Ok(mut e) = commands.get_entity(entity) {
                e.despawn();
            }
        }
    }
}

/// Helper function for background tile loading
fn load_tile_from_path(path: std::path::PathBuf, coord: TileCoord) -> Result<crate::tile::TileData, String> {
    use byteorder::{BigEndian, ReadBytesExt};
    use std::io::Cursor;

    let data = std::fs::read(&path)
        .map_err(|e| format!("Failed to read tile file: {}", e))?;

    let expected_size = 3601 * 3601 * 2;
    if data.len() != expected_size {
        return Err(format!("Invalid tile size: expected {}, got {}", expected_size, data.len()));
    }

    let mut tile = crate::tile::TileData::new(coord, 3601);
    let mut cursor = Cursor::new(data);
    for y in 0..3601 {
        for x in 0..3601 {
            tile.heights[y * 3601 + x] = cursor
                .read_i16::<BigEndian>()
                .map_err(|e| format!("Failed to parse: {}", e))?;
        }
    }
    Ok(tile)
}

/// System to handle completed background tile loads
pub fn process_tile_loads(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut TileLoadTask)>,
    mut cache: ResMut<TileCache>,
) {
    for (entity, mut load_task) in &mut tasks {
        if let Some(result) = future::block_on(future::poll_once(&mut load_task.task)) {
            let coord = load_task.coord;
            match result {
                Ok(data) => {
                    //info!("Loaded tile from disk: {:?}", coord);
                    cache.insert_tile(coord, TileState::Loaded(std::sync::Arc::new(data)));
                }
                Err(e) => {
                    //warn!("Failed to load tile or missing: {}. Using empty tile.", e);
                    // Create empty tile (flat ground)
                    let empty_data = crate::tile::TileData::new(coord, 3601);
                    cache.insert_tile(coord, TileState::Loaded(std::sync::Arc::new(empty_data)));
                }
            }
            commands.entity(entity).despawn();
        }
    }
}

/// System to queue mesh generation tasks
pub fn mesh_update_system(
    mut commands: Commands,
    cache: Res<TileCache>,
    mesh_cache: Res<MeshCache>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    colormap: Res<ColorMap>,
    lod_manager: Res<LodManager>,
    tile_query: Query<(Entity, &TerrainTile, Has<NeedsRegen>)>,
    task_query: Query<(Entity, &MeshGenTask)>,
    tile_load_tasks: Query<Entity, With<TileLoadTask>>,
    radars: Res<crate::radar::Radars>,
    camera: Single<&Transform, With<Camera>>,
    mut last_radar_params: Local<Option<(f32, f64, usize)>>,
    mut last_loaded_tile_count: Local<usize>,
    // Shared material handle — created once, reused every frame to avoid per-tile allocations
    mut shared_material: Local<Option<Handle<StandardMaterial>>>,
) {
    // Get or create the one shared terrain material
    let mat_handle = shared_material.get_or_insert_with(|| {
        materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.8,
            metallic: 0.0,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    }).clone();

    let current_params = (radars.target_altitude_agl, radars.target_rcs, radars.stations.len());
    let params_changed = last_radar_params.map_or(true, |p| p != current_params);

    // Invalidate when the count of *loaded* tiles increases — a tile that just finished loading
    // may fill a gap that previous raycasts skipped, revealing previously hidden terrain.
    // We deliberately ignore mark_loading() mutations (tiles entering Loading state carry no
    // height data yet) so we don't trigger a cascade of NeedsRegen on every queued tile.
    let loaded_tile_count = cache.tiles.values()
        .filter(|s| matches!(s, TileState::Loaded(_)))
        .count();
    let terrain_changed = loaded_tile_count > *last_loaded_tile_count;
    if terrain_changed {
        *last_loaded_tile_count = loaded_tile_count;
    }

    // --- Cancel stale in-flight tasks ---
    // When params change, any running raycasting task was started with the old parameters.
    // Letting them finish wastes CPU and produces a stale mesh that is immediately discarded.
    // We despawn them now and exclude their coords from pending_task_set so that fresh tasks
    // can be spawned for those coords in the same frame (no one-frame hole).
    let mut cancelled_coords: HashSet<TileCoord> = HashSet::new();
    if params_changed || terrain_changed {
        for (task_entity, task) in task_query.iter() {
            if task.radar_params != current_params {
                commands.entity(task_entity).despawn();
                cancelled_coords.insert(task.coord);
            }
        }
    }

    // Build O(1) lookup maps once — avoid O(n²) .find() inside the tile loop.
    // Exclude cancelled coords from pending_task_set so freed slots can be refilled this frame.
    let existing_tile_map: HashMap<TileCoord, (Entity, usize, (f32, f64, usize), bool)> = tile_query
        .iter()
        .map(|(e, t, regen)| (t.coord, (e, t.lod, t.radar_params, regen)))
        .collect();
    let pending_task_set: HashSet<TileCoord> = task_query
        .iter()
        .filter(|(_, t)| !cancelled_coords.contains(&t.coord))
        .map(|(_, t)| t.coord)
        .collect();

    if params_changed || terrain_changed {
        *last_radar_params = Some(current_params);
        for (entity, ..) in existing_tile_map.values() {
            if let Ok(mut e) = commands.get_entity(*entity) {
                e.insert(NeedsRegen);
            }
        }
    }

    if lod_manager.is_changed() {
        for (entity, ..) in existing_tile_map.values() {
            if let Ok(mut e) = commands.get_entity(*entity) {
                e.insert(NeedsRegen);
            }
        }
    }

    let camera_pos = camera.translation;
    let cam_forward = camera.forward();

    // Clone shared data ONCE per frame — all tasks for this frame share them via Arc.
    // Previously each task called `radars.clone()` independently: O(N_stations) heap allocations
    // per tile (including String names). With hundreds of SAM sites that serialised spawning
    // and hammered the allocator. Now: one full clone + one Arc pointer-copy per task.
    let shared_radars  = Arc::new(radars.clone());
    let shared_colormap = Arc::new(colormap.clone());

    let mut snapshot: Option<Arc<HashMap<TileCoord, Arc<crate::tile::TileData>>>> = None;
    let mut candidates = Vec::new();

    for (coord, tile_state) in cache.tiles.iter() {
        if let TileState::Loaded(data_arc) = tile_state {
            let tile_size = 3601.0_f32;
            let center_x = (coord.lon as f32 + 0.5) * tile_size;
            let center_z = -((coord.lat as f32 + 0.5) * tile_size);
            let tile_center = Vec3::new(center_x, 0.0, center_z);

            let distance  = camera_pos.distance(tile_center);
            let lod_level = lod_manager.calculate_lod(distance);

            let needs_regen = match (existing_tile_map.get(coord), pending_task_set.contains(coord)) {
                (None, false) => true,
                (Some((_, tile_lod, tile_params, stale)), false) => {
                    *stale || *tile_lod != lod_level || *tile_params != current_params
                }
                _ => false,
            };

            if needs_regen {
                let dir_to_tile  = (tile_center - camera_pos).normalize_or_zero();
                let dot          = cam_forward.dot(dir_to_tile);

                // --- Off-screen deferral ---
                // Tiles firmly behind the camera that already have a rendered mesh can wait.
                // Their NeedsRegen / stale flag is left intact so they are automatically
                // re-queued the frame the camera rotates toward them.
                // Exception: tiles with no mesh yet (dot threshold ignored — no blank patches)
                // and tiles close enough that they may become visible with a small rotation.
                const DEFER_DISTANCE: f32 = 7_200.0; // ~2 tile widths
                let already_rendered = existing_tile_map.contains_key(coord);
                if dot < -0.3 && distance > DEFER_DISTANCE && already_rendered {
                    continue;
                }

                // --- Frustum weight ---
                // dot = 1.0 : dead-ahead; dot = 0.0 : 90° off; dot = -1.0 : behind.
                // Weights are intentionally extreme so visible tiles always win.
                let frustum_w = if dot > 0.85 { 0.02 }  // centre of view  (~±32°)
                                else if dot > 0.5  { 0.15 }  // in typical FOV  (~±60°)
                                else if dot > 0.0  { 1.0  }  // forward hemi
                                else               { 12.0 }; // behind camera — last resort

                // --- Radar-coverage weight ---
                // Pre-compute here (once) so it factors into the score AND is reused
                // later to skip calling get_relevant_radars a second time.
                let relevant_radars = radars.get_relevant_radars(*coord);
                let radar_w = if !relevant_radars.is_empty() { 0.6 } else { 1.4 };

                let score = distance * frustum_w * radar_w;
                candidates.push((score, *coord, data_arc.clone(), lod_level, distance, relevant_radars));
            }
        }
    }

    // Stable sort: ties keep their original (arbitrary HashMap) order
    candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Two-pass dispatch:
    //   Pass 1 — apply every cache hit in priority order, no cap (handle clone is essentially free).
    //   Pass 2 — spawn raycasting tasks for all cache misses.  No artificial per-frame cap:
    //            the thread pool (size ≈ num_cpus) manages actual concurrency automatically.
    //            Queued-but-not-yet-running tasks cost nothing on the CPU.
    //            Shared Arc<Radars> makes spawning O(1) per task regardless of station count.

    let tile_size = 3601.0_f32;

    // Pass 1: cache hits — apply all, uncapped
    for (_score, coord, _data_arc, lod_level, distance, _relevant_radars) in &candidates {
        let cache_key = make_cache_key(*coord, *lod_level, current_params);
        if let Some(mesh_handle) = mesh_cache.get(&cache_key) {
            debug!("Mesh cache hit {:?} (lod={}, dist={:.0})", coord, lod_level, distance);

            if let Some((old_entity, ..)) = existing_tile_map.get(coord) {
                if let Ok(mut e) = commands.get_entity(*old_entity) {
                    e.despawn();
                }
            }

            commands.spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(mat_handle.clone()),
                Transform::from_xyz(
                    coord.lon as f32 * tile_size,
                    0.0,
                    -((coord.lat + 1) as f32) * tile_size,
                ),
                TerrainTile { coord: *coord, lod: *lod_level, radar_params: current_params },
            ));
        }
    }

    // Guard: wait for all in-flight disk reads to finish before raycasting.
    //
    // Each TileLoadTask entity represents one .hgt file being read in the background.
    // Until it completes, its tile is absent from the snapshot. A raycast that crosses
    // such a tile silently skips those terrain samples, treating the area as perfectly
    // flat — producing false-positive visibility (green where the terrain actually blocks).
    //
    // We block *new* raycasting tasks here; pass-1 cache hits above are unaffected so
    // already-valid meshes keep rendering. NeedsRegen flags are preserved so every tile
    // is retried on the next frame once all reads have settled.
    if !tile_load_tasks.is_empty() {
        return;
    }

    // Pass 2: cache misses — spawn raycasting tasks for all candidates
    for (_score, coord, data_arc, lod_level, distance, relevant_radars) in candidates {
        let cache_key = make_cache_key(coord, lod_level, current_params);
        if mesh_cache.get(&cache_key).is_some() {
            continue; // already handled in pass 1
        }

        if snapshot.is_none() {
            snapshot = Some(Arc::new(cache.get_snapshot()));
        }

        debug!("Queued raycast task {:?} (dist={:.0}, radars={:?})", coord, distance, relevant_radars.len());

        let thread_pool   = AsyncComputeTaskPool::get();
        let data          = data_arc.clone();
        // Arc clones — O(1) pointer increments, not deep copies
        let radars_arc    = shared_radars.clone();
        let colormap_arc  = shared_colormap.clone();
        let snap          = snapshot.as_ref().unwrap().clone();

        let task = thread_pool.spawn(async move {
            let builder = TerrainMeshBuilder::new(lod_level);
            Arc::new(builder.build_mesh(
                &data,
                &*colormap_arc,
                Some(&*radars_arc),
                Some(snap.as_ref()),
                Some(&relevant_radars),
            ))
        });

        commands.spawn(MeshGenTask { task, coord, lod: lod_level, radar_params: current_params });

        if let Some((entity, ..)) = existing_tile_map.get(&coord) {
            if let Ok(mut e) = commands.get_entity(*entity) {
                e.remove::<NeedsRegen>();
            }
        }
    }
}

pub fn process_mesh_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut MeshGenTask)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mesh_cache: ResMut<MeshCache>,
    existing_tiles: Query<(Entity, &TerrainTile, Has<NeedsRegen>)>,
    radars: Res<crate::radar::Radars>,
    mut shared_material: Local<Option<Handle<StandardMaterial>>>,
) {
    // Reuse the same material handle created in mesh_update_system's Local, or make one here
    let mat_handle = shared_material.get_or_insert_with(|| {
        materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.8,
            metallic: 0.0,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    }).clone();

    // Build O(1) lookup map for existing tiles
    let existing_map: HashMap<TileCoord, (Entity, bool)> = existing_tiles
        .iter()
        .map(|(e, t, regen)| (t.coord, (e, regen)))
        .collect();

    let current_params = (radars.target_altitude_agl, radars.target_rcs, radars.stations.len());

    // Upload all completed tasks every frame for fastest UI response.
    // 256 is effectively uncapped given tile_radius ≤ 7 (max ~225 tiles).
    const MAX_UPLOADS_PER_FRAME: usize = 256;
    let mut uploads = 0;

    for (task_entity, mut mesh_task) in &mut tasks {
        if uploads >= MAX_UPLOADS_PER_FRAME {
            break;
        }

        // Safety net for the same-frame race: mesh_update_system despawns stale task entities
        // via deferred commands, so they may still appear in this query within the same frame.
        // Discard the result rather than uploading a mesh that will be immediately invalidated.
        if mesh_task.radar_params != current_params {
            commands.entity(task_entity).despawn();
            continue;
        }

        if let Some(cached) = future::block_on(future::poll_once(&mut mesh_task.task)) {
            let coord        = mesh_task.coord;
            let lod          = mesh_task.lod;
            let radar_params = mesh_task.radar_params;

            // Upload mesh once, store the handle so future cache hits pay zero GPU cost
            let mesh_handle = meshes.add(cached.to_mesh());
            let cache_key   = make_cache_key(coord, lod, radar_params);
            mesh_cache.insert(cache_key, mesh_handle.clone());
            uploads += 1;

            let mut preserve_regen = false;
            if let Some((tile_entity, has_regen)) = existing_map.get(&coord) {
                if *has_regen { preserve_regen = true; }
                if let Ok(mut e) = commands.get_entity(*tile_entity) {
                    e.despawn();
                }
            }

            let tile_size = 3601.0_f32;
            let x_offset  = coord.lon as f32 * tile_size;
            let z_offset  = -((coord.lat + 1) as f32) * tile_size;

            let mut entity_cmds = commands.spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(mat_handle.clone()),
                Transform::from_xyz(x_offset, 0.0, z_offset),
                TerrainTile { coord, lod, radar_params },
            ));

            if preserve_regen {
                entity_cmds.insert(NeedsRegen);
            }

            commands.entity(task_entity).despawn();
        }
    }
}

fn spawn_tile_entity(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    colormap: &ColorMap,
    lod_manager: &LodManager,
    radars: Option<&crate::radar::Radars>,
    cache: Option<&TileCache>,
    coord: TileCoord,
    tile_data: Option<&crate::tile::TileData>,
) {
    let lod = lod_manager.current_level;
    let builder = TerrainMeshBuilder::new(lod);
    let snapshot = cache.map(|c| c.get_snapshot());
    
    let mesh_data = if let Some(data) = tile_data {
        builder.build_mesh(data, colormap, radars, snapshot.as_ref(), None)
    } else {
        builder.build_missing_mesh()
    };
    let mesh = mesh_data.to_mesh();

    let tile_size = 3601.0;
    let x_offset = coord.lon as f32 * tile_size;
    let z_offset = -((coord.lat + 1) as f32) * tile_size;
    
    let radar_params = radars.map(|r| (r.target_altitude_agl, r.target_rcs, r.stations.len())).unwrap_or((0.0, 0.0, 0));

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.8,
            metallic: 0.0,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(x_offset, 0.0, z_offset),
        TerrainTile { coord, lod, radar_params },
    ));

    info!("Spawned tile entity: {:?}", coord);
}
