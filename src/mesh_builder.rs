// Triangle mesh generation for terrain
use crate::colormap::ColorMap;
use crate::tile::TileData;
use bevy::prelude::*;
use bevy::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;
use std::collections::HashMap;
use std::sync::Arc;
use crate::tile::TileCoord;

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

    /// Build a mesh for a given tile
    pub fn build_mesh(&self, tile: &TileData, colormap: &ColorMap, radar: Option<&crate::radar::Radar>, cache_snapshot: Option<&HashMap<TileCoord, Arc<TileData>>>) -> Mesh {
        let step = self.lod_level;
        let size = tile.size;
        
        // Calculate number of vertices (excluding last row/column)
        let max_coord = size - 1;
        let grid_size = (max_coord - 1) / step + 1;
        
        // We need to generate vertices up to max_coord inclusive
        let vertices_per_row = max_coord / step + 1;
        
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
        
        use rayon::prelude::*;
        
        let vertices: Vec<( [f32; 3], [f32; 4] )> = (0..total_vertices)
            .into_par_iter()
            .map(|i| {
                let yi = i / vertices_per_row;
                let xi = i % vertices_per_row;
                
                let y = yi * step;
                let x = xi * step;
                
                let height = tile.get_height(x, y).unwrap_or(0) as f32;
                
                // Position
                let px = (x as f32 / max_coord as f32) * (size as f32) * self.scale;
                let py = height * self.height_scale;
                let pz = (y as f32 / max_coord as f32) * (size as f32) * self.scale;
                
                let position = [px, py, pz];
                
                // Determine color
                let mut final_color_rgba = [1.0, 1.0, 1.0, 1.0];
                
                if let Some(r) = radar {
                    // Re-calculate lat/lon per vertex
                    let v_lat = (tile_lat_base + 1.0) - (y as f64 / max_coord as f64);
                    let v_lon = tile_lon_base + (x as f64 / max_coord as f64);
                    
                    let visible = if let Some(c) = cache_snapshot {
                        r.is_visible_raycast(v_lat, v_lon, height as f32, c)
                    } else {
                        r.is_visible(v_lat, v_lon, height as f32)
                    };

                    if visible {
                         // Green for visible
                        final_color_rgba = [0.0, 1.0, 0.0, 0.3];
                    } else {
                        // Red for hidden
                        final_color_rgba = [1.0, 0.0, 0.0, 0.3];
                    }
                } else {
                     // Fallback to colormap if no radar
                    let c = colormap.get_color(height).to_srgba();
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
        
        // Dummy normals for wireframe (Unlit material doesn't use them, but shader expects attribute)
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
        
        // Build mesh as LineList for wireframe
        let mut mesh = Mesh::new(PrimitiveTopology::LineList, Default::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));
        
        mesh
    }

    /// Build a placeholder mesh for missing tiles (red at height 0)
    pub fn build_missing_mesh(&self) -> Mesh {
        let size = 100; // Simple low-res grid for missing tiles
        let step = self.lod_level.max(10);
        let grid_size = size / step + 1;
        
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
        
        let mut mesh = Mesh::new(PrimitiveTopology::LineList, Default::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));
        
        mesh
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
