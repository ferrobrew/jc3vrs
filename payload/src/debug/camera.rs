//! Per-eye render-camera snapshots: data + capture only (the egui display lives in `ui::render`).
//! Each eye's `CameraManager::m_RenderCamera` projection state is captured after its Draw so the two
//! eyes can be compared, isolating the eye-1 projection corruption.

use parking_lot::Mutex;

/// Snapshot of the render camera's projection state, captured after each eye's Draw so the two eyes
/// can be compared in the debug UI (to isolate the eye-1 projection corruption).
#[derive(Copy, Clone)]
pub(crate) struct CameraSnapshot {
    pub valid: bool,
    pub camera_ptr: usize,
    pub state_bits: u8,
    pub offcenter_tiles: i32,
    pub offcenter_tile_x: i32,
    pub offcenter_tile_y: i32,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub width: i32,
    pub height: i32,
    pub projection: [f32; 16],
    pub view: [f32; 16],
    pub view_proj_f: [f32; 16],
    pub transform: [f32; 16],
}
impl CameraSnapshot {
    const fn empty() -> Self {
        Self {
            valid: false,
            camera_ptr: 0,
            state_bits: 0,
            offcenter_tiles: 0,
            offcenter_tile_x: 0,
            offcenter_tile_y: 0,
            fov: 0.0,
            near: 0.0,
            far: 0.0,
            aspect: 0.0,
            width: 0,
            height: 0,
            projection: [0.0; 16],
            view: [0.0; 16],
            view_proj_f: [0.0; 16],
            transform: [0.0; 16],
        }
    }
}

/// Per-eye render-camera snapshots (index 0 / 1), filled by [`capture_render_camera`].
pub(crate) static CAMERA_SNAPSHOTS: Mutex<[CameraSnapshot; 2]> =
    Mutex::new([CameraSnapshot::empty(), CameraSnapshot::empty()]);

/// Snapshot `CameraManager::m_RenderCamera` into slot `index`. Call after the eye's Draw has been
/// drained, so the captured projection is the one that eye actually rendered with.
pub(crate) fn capture_render_camera(index: usize) {
    unsafe {
        let Some(cm) = jc3gi::camera::camera_manager::CameraManager::get() else {
            return;
        };
        let Some(cam) = cm.m_RenderCamera.as_ref() else {
            return;
        };
        let snap = CameraSnapshot {
            valid: true,
            camera_ptr: cm.m_RenderCamera as usize,
            state_bits: cam.m_StateBitfield.bits(),
            offcenter_tiles: cam.m_OffCenterTiles,
            offcenter_tile_x: cam.m_OffCenterTileX,
            offcenter_tile_y: cam.m_OffCenterTileY,
            fov: cam.m_FOV,
            near: cam.m_Near,
            far: cam.m_Far,
            aspect: cam.m_AspectRatio,
            width: cam.m_Width,
            height: cam.m_Height,
            projection: cam.m_Projection.data,
            view: cam.m_View.data,
            view_proj_f: cam.m_ViewProjectionF.data,
            transform: cam.m_TransformF.data,
        };
        if let Some(slot) = CAMERA_SNAPSHOTS.lock().get_mut(index) {
            *slot = snap;
        }
    }
}
