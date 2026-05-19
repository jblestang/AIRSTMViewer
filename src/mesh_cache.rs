use std::collections::HashMap;
use bevy::prelude::*;
use crate::tile::TileCoord;

/// Cache key: (tile coord, lod step, coverage_revision)
pub type MeshCacheKey = (TileCoord, usize, u64);

pub fn make_cache_key(coord: TileCoord, lod: usize, coverage_revision: u64) -> MeshCacheKey {
    (coord, lod, coverage_revision)
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

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
