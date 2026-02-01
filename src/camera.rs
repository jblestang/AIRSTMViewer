// Camera controller for terrain navigation
use bevy::prelude::*;
use bevy::input::mouse::{MouseMotion, MouseWheel};

/// Camera controller component
#[derive(Component)]
pub struct TerrainCamera {
    pub move_speed: f32,
    pub rotate_speed: f32,
    pub zoom_speed: f32,
}

impl Default for TerrainCamera {
    fn default() -> Self {
        Self {
            move_speed: 2000.0,
            rotate_speed: 0.003,
            zoom_speed: 100.0,
        }
    }
}

/// Update camera based on input
pub fn camera_flight_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut scroll_events: EventReader<MouseWheel>,
    mut query: Query<(&mut Transform, &TerrainCamera)>,
) {
    let Ok((mut transform, camera)) = query.get_single_mut() else {
        return;
    };

    let dt = time.delta_secs();

    // Mouse rotation (right-click drag)
    if mouse_button.pressed(MouseButton::Right) {
        for event in mouse_motion.read() {
            // Rotate around Y axis (yaw)
            let yaw = Quat::from_rotation_y(-event.delta.x * camera.rotate_speed);
            // Rotate around local X axis (pitch)
            let pitch = Quat::from_rotation_x(-event.delta.y * camera.rotate_speed);
            
            transform.rotation = yaw * transform.rotation * pitch;
        }
    } else {
        // Consume events even when not using them
        mouse_motion.clear();
    }

    // Check for modifier keys (Command/Super)
    let super_pressed = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight);

    // Keyboard rotation (Command + Arrows)
    let mut rotation_delta = Vec2::ZERO;
    if super_pressed {
        if keys.pressed(KeyCode::ArrowLeft) { rotation_delta.x += 1.0; }
        if keys.pressed(KeyCode::ArrowRight) { rotation_delta.x -= 1.0; }
        if keys.pressed(KeyCode::ArrowUp) { rotation_delta.y += 1.0; }
        if keys.pressed(KeyCode::ArrowDown) { rotation_delta.y -= 1.0; }
        
        if rotation_delta != Vec2::ZERO {
            // Apply rotation with same speed as mouse
            let yaw = Quat::from_rotation_y(rotation_delta.x * camera.rotate_speed * 10.0); // Multiplier for keys
            let pitch = Quat::from_rotation_x(rotation_delta.y * camera.rotate_speed * 10.0);
            transform.rotation = yaw * transform.rotation * pitch;
        }
    }

    // WASD / Arrow keys for movement
    let mut direction = Vec3::ZERO;
    
    // W / ArrowUp (only if not rotating)
    if keys.pressed(KeyCode::KeyW) || (!super_pressed && keys.pressed(KeyCode::ArrowUp)) {
        direction += transform.forward().as_vec3();
    }
    // S / ArrowDown
    if keys.pressed(KeyCode::KeyS) || (!super_pressed && keys.pressed(KeyCode::ArrowDown)) {
        direction -= transform.forward().as_vec3();
    }
    // A / ArrowLeft
    if keys.pressed(KeyCode::KeyA) || (!super_pressed && keys.pressed(KeyCode::ArrowLeft)) {
        direction -= transform.right().as_vec3();
    }
    // D / ArrowRight
    if keys.pressed(KeyCode::KeyD) || (!super_pressed && keys.pressed(KeyCode::ArrowRight)) {
        direction += transform.right().as_vec3();
    }
    
    // Q/E or Shift/Space for vertical movement
    // Q/Shift = Up (Ascend)
    if keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::ShiftLeft) {
        direction.y += 1.0;
    }
    // E/Space = Down (Descend)
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) {
        direction.y -= 1.0;
    }
    
    // Normalize and apply movement
    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        
        // Boost speed with Shift (if not rotating/using shift for up)
        // Actually Shift is used for UP. Ctrl for speed? 
        // Let's stick to base speed for now.
        
        transform.translation += direction * camera.move_speed * dt;
    }
    
    // Mouse wheel zoom (move forward/backward) along view vector
    for event in scroll_events.read() {
        let forward = transform.forward().as_vec3();
        transform.translation += forward * event.y * camera.zoom_speed;
    }
    
    // Keep camera above ground
    transform.translation.y = transform.translation.y.max(100.0);
}

/// Setup camera
pub fn setup_camera(mut commands: Commands) {
    // Center on France/Italy region (N41E002 area)
    // North = -Z
    let tile_size = 3601.0;
    let center_x = 2.0 * tile_size;  // Longitude 2°E
    let center_z = -42.0 * tile_size; // Latitude 41°N (approx center of tile N41 is at -(41+1) offset + half size? No let's aim for 42N so -42)
    
    // Position camera SOUTH of target (More Positive Z), looking NORTH (Negative Z)
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(center_x, 15000.0, center_z + 5000.0) // South of target, looking down/north
            .looking_at(Vec3::new(center_x, 0.0, center_z - 5000.0), Vec3::Y),
        TerrainCamera::default(),
    ));
}
