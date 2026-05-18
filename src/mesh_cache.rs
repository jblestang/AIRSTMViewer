use std::collections::HashMap;
use bevy::prelude::*;
use crate::tile::TileCoord;

/// Cache key: (tile coord, lod step, alt_bits, rcs_bits, station_count)
/// f32/f64 are stored as raw bits to make them Hash + Eq without extra dependencies.
pub type MeshCacheKey = (TileCoord, usize, u32, u64, usize);

pub fn make_cache_key(coord: TileCoord, lod: usize, radar_params: (f32, f64, usize)) -> MeshCacheKey {
    (coord, lod, radar_params.0.to_bits(), radar_params.1.to_bits(), radar_params.2)
}

/// Stores `Handle<Mesh>` keyed by (coord, lod, radar_params).
///
/// Bevy's `Assets<Mesh>` is reference-counted: as long as this cache holds a `Handle`,
/// the GPU buffer is kept alive across entity despawn/respawn cycles.
/// A cache hit therefore costs only an atomic ref-count clone — no vertex data copy
/// and no GPU re-upload.
#[derive(Resource, Default)]
pub struct MeshCache {
    entries: HashMap<MeshCacheKey, Handle<Mesh>>,
}

impl MeshCache {
    /// Returns a cloned handle (keeps the GPU buffer alive, essentially free).
    pub fn get(&self, key: &MeshCacheKey) -> Option<Handle<Mesh>> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&mut self, key: MeshCacheKey, handle: Handle<Mesh>) {
        self.entries.insert(key, handle);
    }

    /// Drop all cached handles for a tile (called on tile eviction).
    pub fn evict_tile(&mut self, coord: &TileCoord) {
        self.entries.retain(|(c, ..), _| c != coord);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
