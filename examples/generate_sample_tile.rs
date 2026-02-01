// Generate a sample SRTM tile for testing
use std::fs;
use std::io::Write;
use byteorder::{BigEndian, WriteBytesExt};

fn main() {
    // Use local "assets" directory in the project
    let current_dir = std::env::current_dir().expect("Could not determine current directory");
    let cache_dir = current_dir.join("assets");
    fs::create_dir_all(&cache_dir).expect("Failed to create assets dir");

    // Generate a sample tile N00E000.hgt (equator, prime meridian)
    let filename = cache_dir.join("N00E000.hgt");
    
    println!("Generating sample SRTM tile: {}", filename.display());
    
    let size = 3601;
    let mut data = Vec::new();
    
    // Create a simple terrain: cone shape centered in the tile
    let center = size / 2;
    let max_height = 2000i16; // 2000 meters peak
    
    for y in 0..size {
        for x in 0..size {
            // Calculate distance from center
            let dx = (x as i32 - center as i32) as f32;
            let dy = (y as i32 - center as i32) as f32;
            let dist = (dx * dx + dy * dy).sqrt();
            let max_dist = (center as f32) * 1.414; // diagonal
            
            // Height decreases with distance (cone shape)
            let height = if dist < max_dist {
                (max_height as f32 * (1.0 - dist / max_dist)) as i16
            } else {
                0
            };
            
            data.write_i16::<BigEndian>(height).expect("Failed to write");
        }
    }
    
    fs::write(&filename, data).expect("Failed to write tile file");
    println!("Sample tile created successfully!");
    println!("Peak height: {} meters", max_height);
    println!("Tile size: {}x{} samples", size, size);
}
