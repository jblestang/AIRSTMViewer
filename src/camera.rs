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
    let Ok((mut transform, camera)) = query.single_mut() else {
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

    // Check for Shift key (used for switching between Translation and Zoom/Rotate)
    let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    
    // Reset Camera (R)
    if keys.just_pressed(KeyCode::KeyR) {
        let tile_size = 3601.0;
        let center_x = 7.42639 * tile_size;
        let center_z = -43.77528 * tile_size;
        
        *transform = Transform::from_xyz(center_x, 15000.0, center_z + 5000.0)
            .looking_at(Vec3::new(center_x, 0.0, center_z - 5000.0), Vec3::Y);
        info!("Camera reset to home position");
        return;
    }

    // WASD Movement (Standard - Keep as fallback/alternative)
    let mut direction = Vec3::ZERO;
    let mut rotation_yaw = 0.0;
    let mut rotation_pitch = 0.0;
    
    if keys.pressed(KeyCode::KeyW) { direction += transform.forward().as_vec3(); }
    if keys.pressed(KeyCode::KeyS) { direction -= transform.forward().as_vec3(); }
    if keys.pressed(KeyCode::KeyA) { direction -= transform.right().as_vec3(); }
    if keys.pressed(KeyCode::KeyD) { direction += transform.right().as_vec3(); }

    // Logic for Arrow Keys
    if shift_pressed {
        // Shift + Arrows = View Rotation (Pitch/Yaw)
        
        // Shift + Up = Look Up (Pitch +)
        if keys.pressed(KeyCode::ArrowUp) {
            rotation_pitch += 1.0;
        }
        // Shift + Down = Look Down (Pitch -)
        if keys.pressed(KeyCode::ArrowDown) {
            rotation_pitch -= 1.0;
        }
        // Shift + Left = Turn Right (Yaw -)
        if keys.pressed(KeyCode::ArrowLeft) {
            rotation_yaw -= 1.0;
        }
        // Shift + Right = Turn Left (Yaw +)
        if keys.pressed(KeyCode::ArrowRight) {
            rotation_yaw += 1.0;
        }
    } else {
        // Plain Arrows = Translate (Screen Plane/Altitude)
        
        // Up = Translate Up (Altitude +)
        if keys.pressed(KeyCode::ArrowUp) {
            direction.y += 1.0;
        }
        // Down = Translate Down (Altitude -)
        if keys.pressed(KeyCode::ArrowDown) {
            direction.y -= 1.0;
        }
        // Left = Translate Left
        if keys.pressed(KeyCode::ArrowLeft) {
            direction -= transform.right().as_vec3();
        }
        // Right = Translate Right
        if keys.pressed(KeyCode::ArrowRight) {
            direction += transform.right().as_vec3();
        }
    }

    // Apply Rotation
    if rotation_yaw != 0.0 || rotation_pitch != 0.0 {
        let yaw = Quat::from_rotation_y(rotation_yaw * camera.rotate_speed * 10.0);
        let pitch = Quat::from_rotation_x(rotation_pitch * camera.rotate_speed * 10.0);
        
        // Yaw is global (applied before current rotation), Pitch is local (applied after)
        // transform.rotation = yaw * transform.rotation * pitch; 
        
        // Actually, for free cam, we often want yaw to be around global Y.
        // And pitch around local X.
        transform.rotation = yaw * transform.rotation * pitch;
    }
    
    // Normalize and apply movement
    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        
        // Boost speed with Control
        let speed_mult = if keys.pressed(KeyCode::ControlLeft) { 5.0 } else { 1.0 };
        
        transform.translation += direction * camera.move_speed * speed_mult * dt;
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
    // Center on Mont Agel (N43.76 E7.42)
    let tile_size = 3601.0;
    let center_x = 7.42639 * tile_size;  // Longitude 7.42639E
    let center_z = -43.77528 * tile_size; // Latitude 43.77528N
    
    // Position camera SOUTH of target (More Positive Z), looking NORTH (Negative Z)
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(center_x, 15000.0, center_z + 5000.0) // South of target, looking down/north
            .looking_at(Vec3::new(center_x, 0.0, center_z - 5000.0), Vec3::Y),
        TerrainCamera::default(),
    ));
}
