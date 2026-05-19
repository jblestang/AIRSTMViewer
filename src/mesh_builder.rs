// Triangle mesh generation for terrain
use crate::colormap::ColorMap;
use crate::tile::TileData;
use bevy::prelude::*;
use bevy::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;
use std::collections::HashMap;
use std::sync::Arc;
use crate::tile::TileCoord;

/// Raw vertex data produced by the mesh builder.
/// Stored in MeshCache so identical (coord, lod, radar_params) combinations never
/// trigger a second raycasting pass; call `to_mesh()` to obtain a ready-to-use Bevy Mesh.
#[derive(Clone)]
pub struct CachedMeshData {
    pub positions: Vec<[f32; 3]>,
    pub colors:    Vec<[f32; 4]>,
    pub normals:   Vec<[f32; 3]>,
    pub indices:   Vec<u32>,
}

impl CachedMeshData {
    pub fn to_mesh(&self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::LineList, Default::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL,   self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR,    self.colors.clone());
        mesh.insert_indices(Indices::U32(self.indices.clone()));
        mesh
    }
}

/// Build a terrain mesh from tile data
pub struct TerrainMeshBuilder {
    pub lod_level: usize,  // Level of detail (1 = full res, 2 = half res, etc.)
    pub scale: f32,        // Horizontal scale factor
    pub height_scale: f32, // Vertical exaggeration
}

impl Default for TerrainMeshBuilder {
    fn default() -> Self {
        Self {
            lod_level: 1,
            scale: 1.0, 
            height_scale: 1.0,
        }
    }
}

impl TerrainMeshBuilder {
    /// Create a new mesh builder with specified LOD
    pub fn new(lod_level: usize) -> Self {
        Self {
            lod_level,
            scale: 1.0, 
            height_scale: 1.0,
        }
    }

    /// Build mesh data for a given tile. Returns `CachedMeshData` which can be stored
    /// in `MeshCache` and cheaply converted to a `Mesh` via `.to_mesh()`.
    pub fn build_mesh(
        &self, 
        tile: &TileData, 
        colormap: &ColorMap, 
        radars: Option<&crate::radar::Radars>, 
        cache_snapshot: Option<&HashMap<TileCoord, Arc<TileData>>>,
        relevant_radars: Option<&[usize]>,
    ) -> CachedMeshData {
        let step = self.lod_level.max(1);
        let size = tile.size;
        let max_coord = size.saturating_sub(1);
        let vertices_per_row = TileData::lod_vertices_per_row(step, size);
        // World units per DEM index — multiply indices first, then scale (avoids per-vertex float division drift).
        let units_per_dem_index = (size as f32 / max_coord.max(1) as f32) * self.scale;
        
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        
        // Tile origin in World Coordinates (lat/lon)
        // Tile N43E007 origin is 43N, 7E.
        // x index 0..3600 maps to 0..1 deg.
        let tile_lat_base = tile.coord.lat as f64;
        let tile_lon_base = tile.coord.lon as f64;
        
        // Generate vertices in parallel using Rayon
        // Generate vertices in parallel using Rayon (Outer loop only to reduce overhead)
        // ALGORITHM: Parallel Grid Generation
        // Instead of nested loops (y, x) which are hard to parallelize efficiently,
        // we flatten the 2D grid into a 1D index space (0..total_vertices).
        // Each index `i` is then mapped back to (x, y) coordinates:
        //   y = i / width
        //   x = i % width
        // This allows Rayon to split the workload evenly across all available CPU cores.
        let total_vertices = vertices_per_row * vertices_per_row;
        
        // Precompute max detection range for every relevant radar — O(N_radars) work done
        // here rather than inside the per-vertex hot loop.
        // Previously calculate_max_range() (3× f64::powf ≈ 300 ns each) was called once
        // per vertex per radar: 8 281 vertices × N_radars = millions of powf calls per tile.
        let radar_ranges: Option<Vec<(usize, f64)>> = match (radars, relevant_radars) {
            (Some(rds), Some(indices)) => Some(
                indices.iter().map(|&idx| {
                    (idx, rds.stations[idx].calculate_max_range(rds.target_rcs))
                }).collect()
            ),
            _ => None,
        };

        use rayon::prelude::*;
        
        let vertices: Vec<( [f32; 3], [f32; 4] )> = (0..total_vertices)
            .into_par_iter()
            .map(|i| {
                let yi = i / vertices_per_row;
                let xi = i % vertices_per_row;

                let (x_start, x_end, y_start, y_end) =
                    tile.lod_dem_bin(xi, yi, step, vertices_per_row);

                // Max-sample full-res DEM for mesh geometry (peaks not lost at LOD > 1).
                let mesh_height = tile
                    .max_height_in_region(x_start, y_start, x_end, y_end)
                    .unwrap_or(0) as f32;

                // Radar / target altitude uses ground at bin centre, not max height.
                // Using max here inflates check_alt on peaks and draws false "ridges" on
                // coverage boundaries (target is AGL above local ground, not above cell max).
                let x_center = (x_start + x_end) / 2;
                let y_center = (y_start + y_end) / 2;
                let ground_m = tile.height_at_dem_index(x_center, y_center) as f32;

                // Vertex at the SW corner of its DEM bin (integer indices → world, no rounding).
                let px = x_start as f32 * units_per_dem_index;
                let py = mesh_height * self.height_scale;
                let pz = y_start as f32 * units_per_dem_index;

                let position = [px, py, pz];

                // Determine color
                let final_color_rgba;

                if let Some(rds) = radars {
                    if let Some(snap) = cache_snapshot {
                        let v_lon =
                            tile_lon_base + TileData::dem_index_to_lon_frac(x_center, max_coord);
                        let v_lat = tile_lat_base
                            + TileData::dem_index_to_lat_frac(y_center, max_coord);
                        let check_alt = ground_m + rds.target_altitude_agl;
                        
                        let mut visible = false;
                        let mut color = None;
                        
                        if let Some(ranges) = &radar_ranges {
                            // Fast path: precomputed max_range avoids powf per vertex.
                            // Haversine is also computed only once inside precomputed variant
                            // (the old code computed it twice: in is_visible + is_visible_raycast).
                            for &(idx, max_range) in ranges {
                                if let Some(radar) = rds.stations.get(idx) {
                                    if radar.is_visible_raycast_precomputed(
                                        v_lat, v_lon, check_alt, max_range, snap,
                                    ) {
                                        visible = true;
                                        color = Some(radar.color);
                                        break;
                                    }
                                }
                            }
                        } else {
                            let (v, c) = rds.check_visibility(v_lat, v_lon, check_alt, snap);
                            visible = v;
                            color = c;
                        }

                        if visible {
                             if let Some(c) = color {
                                let srgba = c.to_srgba();
                                final_color_rgba = [srgba.red, srgba.green, srgba.blue, 0.3];
                             } else {
                                final_color_rgba = [0.0, 1.0, 0.0, 0.3];
                             }
                        } else {
                            final_color_rgba = [1.0, 0.0, 0.0, 0.1];
                        }
                    } else {
                         let c = colormap.get_color(mesh_height).to_srgba();
                         final_color_rgba = [c.red, c.green, c.blue, c.alpha];
                    }
                } else {
                    let c = colormap.get_color(mesh_height).to_srgba();
                    final_color_rgba = [c.red, c.green, c.blue, c.alpha];
                }
                
                (position, final_color_rgba)
            })
            .collect();

        // Populate the buffers
        for (pos, col) in vertices {
            positions.push(pos);
            colors.push(col);
        }
        
        // Generate wireframe indices (optimized: min lines)
        // Grid size is number of cells
        let cell_cols = vertices_per_row - 1;
        let cell_rows = vertices_per_row - 1;
        
        for y in 0..cell_rows {
            for x in 0..cell_cols {
                let i0 = y * vertices_per_row + x;
                let i1 = i0 + 1;
                let i2 = i0 + vertices_per_row;
                // let i3 = i2 + 1; 
                
                // Optimized Wireframe Topology:
                // For each cell (square), we draw 3 lines to form the triangles:
                // 1. Top Edge (i0 -> i1)
                // 2. Left Edge (i0 -> i2)
                // 3. Diagonal (i1 -> i2) - giving the "triangulated" look
                // Right and Bottom edges are handled by the next neighbor's Left/Top, 
                // except for the last row/column which are handled explicitly below.
                indices.push(i0 as u32); indices.push(i1 as u32); // Top (i0-i1)
                indices.push(i0 as u32); indices.push(i2 as u32); // Left (i0-i2)
                indices.push(i1 as u32); indices.push(i2 as u32); // Diagonal (i1-i2)
                
                // If last column, draw Right edge
                if x == cell_cols - 1 {
                    let i3 = i2 + 1;
                     indices.push(i1 as u32); indices.push(i3 as u32); // Right (i1-i3)
                }
                
                // If last row, draw Bottom edge
                if y == cell_rows - 1 {
                    let i3 = i2 + 1;
                     indices.push(i2 as u32); indices.push(i3 as u32); // Bottom (i2-i3)
                }
            }
        }
        
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];

        CachedMeshData { positions, colors, normals, indices }
    }

    /// Build a placeholder mesh for missing tiles (red at height 0)
    pub fn build_missing_mesh(&self) -> CachedMeshData {
        let size = 100; // Simple low-res grid for missing tiles
        let step = self.lod_level.max(10);
        let _grid_size = size / step + 1;
        
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        
        // Generate flat red grid
        for y in (0..=size).step_by(step) {
            for x in (0..=size).step_by(step) {
                // Use absolute coordinates to match terrain tiles
                let px = x as f32 * self.scale;
                let py = 0.0; // Height 0
                let pz = y as f32 * self.scale;
                
                positions.push([px, py, pz]);
                colors.push([1.0, 0.0, 0.0, 1.0]); // Red
            }
        }
        
        // Generate wireframe indices for missing tile
        let grid_w = size / step; // Number of cells
        
        for y in 0..grid_w {
            for x in 0..grid_w {
                let i0 = y * (grid_w + 1) + x;
                let i1 = i0 + 1;
                let i2 = i0 + (grid_w + 1);
                
                // Wireframe lines
                indices.push(i0 as u32); indices.push(i1 as u32); // Top
                indices.push(i0 as u32); indices.push(i2 as u32); // Left
                indices.push(i1 as u32); indices.push(i2 as u32); // Diagonal
                
                // Right and Bottom edges
                if x == grid_w - 1 {
                    let i3 = i2 + 1;
                    indices.push(i1 as u32); indices.push(i3 as u32);
                }
                if y == grid_w - 1 {
                    let i3 = i2 + 1;
                    indices.push(i2 as u32); indices.push(i3 as u32);
                }
            }
        }
        
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];

        CachedMeshData { positions, colors, normals, indices }
    }

    /// Calculate normals for the mesh
    fn calculate_normals(&self, positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
        let mut normals = vec![[0.0f32, 0.0, 0.0]; positions.len()];
        
        // Calculate face normals and accumulate
        for triangle in indices.chunks(3) {
            let i0 = triangle[0] as usize;
            let i1 = triangle[1] as usize;
            let i2 = triangle[2] as usize;
            
            let p0 = Vec3::from(positions[i0]);
            let p1 = Vec3::from(positions[i1]);
            let p2 = Vec3::from(positions[i2]);
            
            let edge1 = p1 - p0;
            let edge2 = p2 - p0;
            let normal = edge1.cross(edge2);
            
            // Accumulate normals
            normals[i0][0] += normal.x;
            normals[i0][1] += normal.y;
            normals[i0][2] += normal.z;
            
            normals[i1][0] += normal.x;
            normals[i1][1] += normal.y;
            normals[i1][2] += normal.z;
            
            normals[i2][0] += normal.x;
            normals[i2][1] += normal.y;
            normals[i2][2] += normal.z;
        }
        
        // Normalize
        for normal in &mut normals {
            let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            if len > 0.0 {
                normal[0] /= len;
                normal[1] /= len;
                normal[2] /= len;
            } else {
                normal[1] = 1.0; // Default to up
            }
        }
        
        normals
    }
}
