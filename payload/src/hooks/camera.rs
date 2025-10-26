use detours_macro::detour;
use jc3gi::camera::camera::Camera;
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CAMERA_UPDATE_RENDER_BINDER)
}

#[detour(address = 0x143_2EB_C70)]
fn camera_update_render(camera: *mut Camera, dt: f32, dtf: f32) {
    // Can override the camera's view by setting m_TransformT0
    // and m_TransformT1 if the camera is the active camera, but
    // this is well after the camera position for gameplay is established,
    // so it's not ideal as an override target. Still, could be useful for
    // matching the headset's view?
    CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
}
