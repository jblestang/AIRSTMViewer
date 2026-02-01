# Quick Start Guide

## Running the Application

1. **Build the project:**
   ```bash
   cd /Users/jean-baptiste/AIRSTMViewer
   cargo build --release
   ```

2. **Generate a sample tile for testing:**
   ```bash
   cargo run --example generate_sample_tile
   ```
   This creates a cone-shaped terrain at `assets/N00E000.hgt`

3. **Run the viewer:**
   ```bash
   cargo run --release
   ```

## Controls

| Action | Keys |
|--------|------|
| Move Forward/Back | W/S or ↑/↓ |
| Move Left/Right | A/D or ←/→ |
| Move Up/Down | E/Q or Space/Shift |
| Rotate Camera | Right-click + Drag |
| Zoom In/Out | Mouse Wheel |

## Adding Real SRTM Data

1. Visit [USGS EarthExplorer](https://earthexplorer.usgs.gov/)
2. Search for your area of interest
3. Download SRTM 1 Arc-Second Global tiles (.hgt files)
4. Place them in the `assets/` directory
5. Restart the application

## What You'll See

- **Green cone**: The sample terrain (N00E000.hgt)
- **Red squares**: Missing tiles (ocean or unavailable areas)
- **Dynamic LOD**: Mesh detail changes as you zoom in/out

## Troubleshooting

**Window doesn't open?**
- Make sure you're running on macOS with Metal support
- Check terminal output for errors

**No terrain visible?**
- The sample tile is at coordinates (0, 0)
- Camera starts at (0, 100, 200) looking at origin
- Try moving the camera with WASD keys

**Performance issues?**
- Reduce LOD by staying further from terrain
- Close other GPU-intensive applications
