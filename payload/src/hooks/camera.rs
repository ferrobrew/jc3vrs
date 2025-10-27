use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    camera::{
        camera::Camera,
        camera_context::{CameraContext, CameraControlContext},
    },
    character::character::Character,
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CAMERA_UPDATE_RENDER_BINDER)
        .with_static_binder(&CAMERA_TREE_UPDATE_RENDER_CONTEXTS_BINDER)
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

#[detour(address = 0x143_705_610)]
fn camera_tree_update_render_contexts(
    tree: *mut c_void,
    camera_control_context: *mut CameraControlContext,
) {
    CAMERA_TREE_UPDATE_RENDER_CONTEXTS
        .get()
        .unwrap()
        .call(tree, camera_control_context);

    unsafe {
        let Some(local_character) = Character::get_local_player_character().as_mut() else {
            return;
        };

        let mut head_position = jc3gi::types::math::Vector3::default();
        local_character.get_head_position(&mut head_position);

        let Some(ccc) = camera_control_context.as_mut() else {
            return;
        };
        patch_context(&mut ccc.m_NextCameraContext, head_position);
        patch_context(&mut ccc.m_NextRenderContext, head_position);
    }

    fn patch_context(context: &mut CameraContext, head_position: Vector3) {
        context.m_CameraTransform.data[12] = head_position.data[0];
        context.m_CameraTransform.data[13] = head_position.data[1];
        context.m_CameraTransform.data[14] = head_position.data[2];
        context.m_AlternateAimTransform = context.m_CameraTransform;
        context.m_ListenerTransform = context.m_CameraTransform;
        context.m_FOV = 90.0_f32.to_radians();
    }
}
