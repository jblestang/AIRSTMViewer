# SRTM 3D Tile Viewer

A 3D terrain viewer for SRTM (Shuttle Radar Topography Mission) elevation data built with Bevy game engine in Rust.

## Features

- **3D Terrain Rendering**: Triangle-based mesh generation from SRTM elevation data
- **Dynamic Tile Loading**: Automatic download and caching of SRTM tiles
- **Level of Detail (LOD)**: Adaptive mesh resolution based on camera distance
- **Interactive Camera**: Full 3D navigation with keyboard and mouse controls
- **Height-based Colormap**: Terrain visualization with elevation-based colors
- **Tile Caching**: Persistent disk cache for downloaded tiles

## Controls

- **WASD / Arrow Keys**: Move camera forward/backward/left/right
- **Q/E or Shift/Space**: Move camera up/down
- **Right-click + Drag**: Rotate camera view
- **Mouse Wheel**: Zoom in/out (adjusts camera speed and height)

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release
```

## How It Works

### Tile System

SRTM tiles are organized in a 1° x 1° grid, named by their southwest corner coordinates (e.g., `N37W122.hgt`). Each tile contains 3601x3601 elevation samples (1 arc-second resolution, ~30m).

### Mesh Generation

The terrain is rendered using triangle meshes (not quads). Each grid cell is split into two triangles for proper 3D rendering. Vertex colors are computed from elevation using a terrain colormap.

### LOD System

The Level of Detail system adjusts mesh resolution based on camera height:
- Close (< 50m): Full resolution (LOD 1)
- Medium (50-100m): Half resolution (LOD 2)
- Far (100-200m): Quarter resolution (LOD 4)
- Very far (> 400m): 1/16 resolution (LOD 16)

### Caching

Downloaded tiles are cached in the local `assets/` directory for fast reloading. The cache persists between sessions.

### Missing Tiles

Tiles that don't exist (e.g., ocean areas) are rendered as flat red squares at height 0.

## Architecture

- `tile.rs`: Tile coordinate system and data structures
- `cache.rs`: Tile cache management and disk I/O
- `downloader.rs`: Async tile downloading (currently placeholder)
- `mesh_builder.rs`: Triangle mesh generation with LOD
- `colormap.rs`: Elevation-to-color mapping
- `lod.rs`: Level of Detail management
- `camera.rs`: Camera controller and input handling
- `systems.rs`: Bevy systems for tile loading and mesh updates
- `main.rs`: Application entry point and setup

## Current Limitations

1. **Download Implementation**: The downloader currently returns "Missing" for all tiles. To use real SRTM data:
   - Download tiles manually from [USGS EarthExplorer](https://earthexplorer.usgs.gov/)
   - Place `.hgt` files in the `assets/` directory
   - Or implement HTTP downloading in `downloader.rs`

2. **Coordinate System**: The viewer currently loads tiles around coordinate (0, 0). You may want to adjust the starting position in `systems.rs`.

## Future Enhancements

- Implement actual SRTM tile downloading from public sources
- Add authentication support for NASA Earthdata
- Improve frustum culling for better performance
- Add texture mapping support
- Implement water rendering for ocean tiles
- Add coordinate display and search functionality
- Support for different SRTM resolutions (3 arc-second, 1 arc-second)

## License

This project is open source. SRTM data is public domain courtesy of NASA/USGS.
