use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    camera::{
        camera::Camera,
        camera_context::{CameraContext, CameraControlContext},
    },
    character::character::{Character, SafeBoneIndex},
    types::math::Matrix4,
};
use parking_lot::Mutex;
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

#[derive(Copy, Clone)]
pub struct CameraSettings {
    pub enabled: bool,
    pub body_offset: glam::Vec3,
    pub head_offset: glam::Vec3,
    pub blurs_enabled: bool,
    pub always_use_t1: bool,
}
impl CameraSettings {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            body_offset: glam::Vec3::new(0.0, 0.1, 0.0),
            head_offset: glam::Vec3::new(0.0, -0.1, 0.0),
            blurs_enabled: false,
            always_use_t1: false,
        }
    }
}
pub static CAMERA_SETTINGS: Mutex<CameraSettings> = Mutex::new(CameraSettings::new());

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
        let Some(ccc) = camera_control_context.as_mut() else {
            return;
        };

        let camera_settings = *CAMERA_SETTINGS.lock();
        if !camera_settings.enabled {
            return;
        }

        let mut head_matrix = Matrix4::default();
        local_character.get_safe_bone_matrix(SafeBoneIndex::HEAD, &mut head_matrix);
        let head_matrix = glam::Mat4::from(head_matrix);

        let character_t0_matrix = glam::Mat4::from(if camera_settings.always_use_t1 {
            local_character.m_WorldMatrixT1
        } else {
            local_character.m_WorldMatrixT0
        });
        let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);

        patch_context(
            &mut ccc.m_PreviousCameraContext,
            calculate_head_position(character_t0_matrix, head_matrix, &camera_settings),
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_PreviousRenderContext,
            calculate_head_position(character_t0_matrix, head_matrix, &camera_settings),
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextCameraContext,
            calculate_head_position(character_t1_matrix, head_matrix, &camera_settings),
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextRenderContext,
            calculate_head_position(character_t1_matrix, head_matrix, &camera_settings),
            camera_settings.blurs_enabled,
        );
    }

    fn calculate_head_position(
        character_matrix: glam::Mat4,
        head_matrix: glam::Mat4,
        camera_settings: &CameraSettings,
    ) -> glam::Vec3 {
        let (_, character_rotation, _character_position) =
            character_matrix.to_scale_rotation_translation();

        let head_worldspace_matrix = character_matrix * head_matrix;
        let (_, head_rotation, mut head_position) =
            head_worldspace_matrix.to_scale_rotation_translation();

        head_position += head_rotation * camera_settings.head_offset;
        head_position += character_rotation * camera_settings.body_offset;

        head_position
    }

    fn patch_context(context: &mut CameraContext, head_position: glam::Vec3, blurs_enabled: bool) {
        context.m_CameraTransform.data[12] = head_position.x;
        context.m_CameraTransform.data[13] = head_position.y;
        context.m_CameraTransform.data[14] = head_position.z;
        context.m_AlternateAimTransform = context.m_CameraTransform;
        context.m_ListenerTransform = context.m_CameraTransform;
        context.m_FOV = 90.0_f32.to_radians();

        if !blurs_enabled {
            context.m_MaxMotionBlur = 0.0;
            context.m_MotionBlurFactor = 0.0_f32;
            context.m_RadialBlurFactor = 0.0;
        }
    }
}
