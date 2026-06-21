#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Action ID, indexing the static action-name-to-ID table (action_name_table at 0x142_D99_370).
/// 255 actions (PAUSE..GUI_USE_BUTTON); the numbering is fixed at build time, so IDs can be
/// hardcoded. The engine takes a raw int for these, and this enum is repr-int, so it transmutes
/// cleanly into the action_id / action parameters.
pub enum Action {
    PAUSE = 0isize as _,
    LOOK_UP = 1isize as _,
    LOOK_DOWN = 2isize as _,
    LOOK_LEFT = 3isize as _,
    LOOK_RIGHT = 4isize as _,
    LOOK_BACK_CAM = 5isize as _,
    VEHICLE_CAM = 6isize as _,
    TOGGLE_MAGNET_1 = 7isize as _,
    TOGGLE_MAGNET_2 = 8isize as _,
    FIRE_VEHICLE_WEAPON_PRIMARY = 9isize as _,
    FIRE_LEFT = 10isize as _,
    FIRE_RIGHT = 11isize as _,
    FIRE_VEHICLE_WEAPON_SECONDARY = 12isize as _,
    THROW_GRENADE = 13isize as _,
    RELOAD = 14isize as _,
    NEXT_WEAPON = 15isize as _,
    PREV_WEAPON = 16isize as _,
    FREEFLY_MOVE_UP = 17isize as _,
    FREEFLY_MOVE_DOWN = 18isize as _,
    FREEFLY_ROLL_LEFT = 19isize as _,
    FREEFLY_ROLL_RIGHT = 20isize as _,
    FREEFLY_ZOOM_IN = 21isize as _,
    FREEFLY_ZOOM_OUT = 22isize as _,
    FREEFLY_TOGGLE_LOCK_HORIZON = 23isize as _,
    FREEFLY_MISC_1 = 24isize as _,
    FREEFLY_MISC_2 = 25isize as _,
    FREEFLY_SPEED_UP = 26isize as _,
    FREEFLY_SLOW_DOWN = 27isize as _,
    MOVE_FORWARD = 28isize as _,
    MOVE_BACKWARD = 29isize as _,
    MOVE_LEFT = 30isize as _,
    MOVE_RIGHT = 31isize as _,
    WALK = 32isize as _,
    JUMP = 33isize as _,
    CHALLENGE_RESET_VEHICLE = 34isize as _,
    ENTER_VEHICLE = 35isize as _,
    USE_ITEM = 36isize as _,
    ACCELERATE = 37isize as _,
    REVERSE = 38isize as _,
    TURN_LEFT = 39isize as _,
    TURN_RIGHT = 40isize as _,
    HANDBRAKE = 41isize as _,
    SOUND_HORN_SIREN = 42isize as _,
    EXIT_VEHICLE = 43isize as _,
    BIKE_TILT_FORWARD = 44isize as _,
    BIKE_TILT_BACKWARD = 45isize as _,
    HELI_FORWARD = 46isize as _,
    HELI_BACKWARD = 47isize as _,
    HELI_TURN_LEFT = 48isize as _,
    HELI_TURN_RIGHT = 49isize as _,
    HELI_ROLL_LEFT = 50isize as _,
    HELI_ROLL_RIGHT = 51isize as _,
    HELI_INC_ALTITUDE = 52isize as _,
    HELI_DEC_ALTITUDE = 53isize as _,
    HELI_AI_TARGET_ALTITUDE = 54isize as _,
    PLANE_PITCH_UP = 55isize as _,
    PLANE_PITCH_DOWN = 56isize as _,
    PLANE_TURN_LEFT = 57isize as _,
    PLANE_TURN_RIGHT = 58isize as _,
    PLANE_ROLL_LEFT = 59isize as _,
    PLANE_ROLL_RIGHT = 60isize as _,
    PLANE_INC_TRUST = 61isize as _,
    PLANE_DEC_TRUST = 62isize as _,
    BOAT_FORWARD = 63isize as _,
    BOAT_BACKWARD = 64isize as _,
    BOAT_TURN_LEFT = 65isize as _,
    BOAT_TURN_RIGHT = 66isize as _,
    MOVE_TO_DRIVERS_SEAT = 67isize as _,
    BIKE_LEAN_LEFT = 68isize as _,
    BIKE_LEAN_RIGHT = 69isize as _,
    DIVE = 70isize as _,
    SWIM = 71isize as _,
    ZOOM_IN = 72isize as _,
    ZOOM_OUT = 73isize as _,
    GUI_OK = 74isize as _,
    GUI_CANCEL = 75isize as _,
    GUI_UP = 76isize as _,
    GUI_DOWN = 77isize as _,
    GUI_LEFT = 78isize as _,
    GUI_RIGHT = 79isize as _,
    GUI_NEXT = 80isize as _,
    GUI_BACK = 81isize as _,
    GUI_PDA_ZOOM_OUT = 82isize as _,
    GUI_PAUSE = 83isize as _,
    XLIVE_LOGOUT = 84isize as _,
    XLIVE_APONLINE = 85isize as _,
    ARROW_UP = 86isize as _,
    ARROW_DOWN = 87isize as _,
    ARROW_LEFT = 88isize as _,
    ARROW_RIGHT = 89isize as _,
    MAP_ZOOM_IN = 90isize as _,
    MAP_ZOOM_OUT = 91isize as _,
    DEBUG_MODE_COMBO1 = 92isize as _,
    DEBUG_MODE_COMBO2 = 93isize as _,
    FREEFLY_COMBO1 = 94isize as _,
    FREEFLY_COMBO2 = 95isize as _,
    SWITCH_WEAPON = 96isize as _,
    END_MOVIE = 97isize as _,
    GUI_LOAD = 98isize as _,
    GUI_SAVE = 99isize as _,
    GUI_CREATE = 100isize as _,
    GUI_DELETE = 101isize as _,
    GUI_DEFAULT = 102isize as _,
    SCREENSHOT = 103isize as _,
    SCREENSHOT_NO_UI = 104isize as _,
    MECH_PUNCH = 105isize as _,
    MECH_FIRE_GRAVITY_WEAPON_PRIMARY = 106isize as _,
    MECH_FIRE_RIGHT_HAND_WEAPON = 107isize as _,
    MECH_FIRE_GRAVITY_WEAPON_SECONDARY = 108isize as _,
    DASH = 109isize as _,
    STUNT_JUMP = 110isize as _,
    GUI_PDA = 111isize as _,
    GUI_PAGE_NEXT = 112isize as _,
    GUI_PAGE_PREV = 113isize as _,
    VEHICLE_DOCKING_YAW = 114isize as _,
    BYPASS_FALL_PREVENTION = 115isize as _,
    SIDESTEP_LEFT_GAIN_LOF = 116isize as _,
    SIDESTEP_LEFT_BREAK_LOF = 117isize as _,
    SIDESTEP_RIGHT_GAIN_LOF = 118isize as _,
    SIDESTEP_RIGHT_BREAK_LOF = 119isize as _,
    EVADE = 120isize as _,
    MECH_JUMP = 121isize as _,
    MECH_CROUCH = 122isize as _,
    HELI_AI_AUTO_ROLL = 123isize as _,
    SEQUENCE_BUTTON1 = 124isize as _,
    SEQUENCE_BUTTON2 = 125isize as _,
    SEQUENCE_BUTTON3 = 126isize as _,
    SEQUENCE_BUTTON4 = 127isize as _,
    MC_FIRE = 128isize as _,
    MC_RELOAD = 129isize as _,
    USE_VEHICLE_MOD = 130isize as _,
    QUICK_SAVE = 131isize as _,
    QUICK_LOAD = 132isize as _,
    DEBUG_INCREE = 133isize as _,
    DEBUG_DECREE = 134isize as _,
    REELED_IN_JUMP_ACTION = 135isize as _,
    REELED_IN_RELEASE_ACTION = 136isize as _,
    LOOK_AT = 137isize as _,
    SKIP_CUTSCENE = 138isize as _,
    EQUIP_BLACK_MARKET_BEACON = 139isize as _,
    ACTIVATE_BLACK_MARKET_BEACON = 140isize as _,
    ACTIVATE_EXTRACTION_BEACON = 141isize as _,
    WINGSUIT_TAKEOFF = 142isize as _,
    WINGSUIT_AIRBRAKE = 143isize as _,
    WINGSUIT_EVADE = 144isize as _,
    MOVE_ALL = 145isize as _,
    WINGSUIT_BOOST = 146isize as _,
    DP_UP = 147isize as _,
    DP_DOWN = 148isize as _,
    DP_LEFT = 149isize as _,
    DP_RIGHT = 150isize as _,
    BTN_Y = 151isize as _,
    BTN_A = 152isize as _,
    BTN_X = 153isize as _,
    BTN_B = 154isize as _,
    L1 = 155isize as _,
    L2 = 156isize as _,
    R1 = 157isize as _,
    R2 = 158isize as _,
    DEBUG_STEP = 159isize as _,
    SUBMERSIBLE_DIVE = 160isize as _,
    SUBMERSIBLE_SURFACE = 161isize as _,
    DEV_MODIFIER_COMBO_1 = 162isize as _,
    DEV_MODIFIER_COMBO_2 = 163isize as _,
    FREEFLY = 164isize as _,
    MEASURE_TOOL = 165isize as _,
    DETONATE_EXPLOSIVE_TAP = 166isize as _,
    PLANT_EXPLOSIVE = 167isize as _,
    DETONATE_EXPLOSIVE = 168isize as _,
    OPEN_WINGSUIT = 169isize as _,
    FIRE_WINGSUIT_WEAPON_MAIN = 170isize as _,
    OPEN_PARACHUTE = 171isize as _,
    FIRE_WINGSUIT_WEAPON_SECONDARY = 172isize as _,
    FIRE_GRAPPLE = 173isize as _,
    PUSH_GRAPPLE = 174isize as _,
    VEHICLE_RELEASE_GRAPPLE = 175isize as _,
    PRECISION_AIM = 176isize as _,
    QUICK_TURN_COMBO_1 = 177isize as _,
    QUICK_TURN_COMBO_2 = 178isize as _,
    START = 179isize as _,
    SELECT = 180isize as _,
    L3 = 181isize as _,
    R3 = 182isize as _,
    SELECT_EXPLOSIVES = 183isize as _,
    PROFILER_UP = 184isize as _,
    PROFILER_DOWN = 185isize as _,
    PROFILER_LEFT = 186isize as _,
    PROFILER_RIGHT = 187isize as _,
    PROFILER_CLOSE = 188isize as _,
    PROFILER_CHANGE_SORTING = 189isize as _,
    PROFILER_SHOW_EMPTY_TIMERS = 190isize as _,
    PROFILER_PAUSE = 191isize as _,
    PROFILER_TOGGLE_INPUT_FOCUS_A = 192isize as _,
    PROFILER_TOGGLE_INPUT_FOCUS_B = 193isize as _,
    RETRACT_GRAPPLE = 194isize as _,
    RELEASE_GRAPPLE = 195isize as _,
    GUI_TAB_NEXT = 196isize as _,
    GUI_TAB_PREV = 197isize as _,
    GUI_SPECIAL_BUTTON = 198isize as _,
    GUI_LSTICK_UP = 199isize as _,
    GUI_LSTICK_DOWN = 200isize as _,
    GUI_LSTICK_LEFT = 201isize as _,
    GUI_LSTICK_RIGHT = 202isize as _,
    GUI_RSTICK_UP = 203isize as _,
    GUI_RSTICK_DOWN = 204isize as _,
    GUI_RSTICK_LEFT = 205isize as _,
    GUI_RSTICK_RIGHT = 206isize as _,
    GUI_ZOOM_IN = 207isize as _,
    GUI_ZOOM_OUT = 208isize as _,
    DRONE_ZOOM_IN_ANALOG = 209isize as _,
    DRONE_ZOOM_IN_DIGITAL = 210isize as _,
    DRONE_ZOOM_OUT_ANALOG = 211isize as _,
    DRONE_ZOOM_OUT_DIGITAL = 212isize as _,
    DRONE_FOCAL_IN_ANALOG = 213isize as _,
    DRONE_FOCAL_OUT_ANALOG = 214isize as _,
    DRONE_FOV_IN = 215isize as _,
    DRONE_FOV_OUT = 216isize as _,
    DRONE_MOVE_LEFT = 217isize as _,
    DRONE_MOVE_RIGHT = 218isize as _,
    DRONE_MOVE_UP = 219isize as _,
    DRONE_MOVE_DOWN = 220isize as _,
    DRONE_LOOK_LEFT = 221isize as _,
    DRONE_LOOK_RIGHT = 222isize as _,
    DRONE_LOOK_UP = 223isize as _,
    DRONE_LOOK_DOWN = 224isize as _,
    DRONE_TOGGLE_FOLLOW = 225isize as _,
    DRONE_CENTER = 226isize as _,
    DRONE_FIRE = 227isize as _,
    DRONE_TAG = 228isize as _,
    DRONE_SWITCH_MODE = 229isize as _,
    DRONE_WEAPON1 = 230isize as _,
    DRONE_WEAPON2 = 231isize as _,
    DRONE_WEAPON3 = 232isize as _,
    DRONE_WEAPON4 = 233isize as _,
    SELECT_DUEL_WIELD = 234isize as _,
    SELECT_TWO_HANDED = 235isize as _,
    SELECT_TWO_HANDED_SPECIAL = 236isize as _,
    SOAKFLY_CAM_ACCELERATE = 237isize as _,
    SOAKFLY_CAM_REVERSE = 238isize as _,
    MOVIE_CAM_TOGGLE_FOV_ROLL = 239isize as _,
    MOVIE_CAM_GAME_SPEED_INC = 240isize as _,
    MOVIE_CAM_GAME_SPEED_DEC = 241isize as _,
    MOVIE_CAM_SHAKE_AMP_DEC = 242isize as _,
    MOVIE_CAM_SHAKE_AMP_INC = 243isize as _,
    MOVIE_CAM_PAUSE = 244isize as _,
    MOVIE_CAM_ATTACH_TO_OBJ = 245isize as _,
    MOVIE_CAM_TOGGLE_MOUNT = 246isize as _,
    TOGGLE_DEV_MENU = 247isize as _,
    CANCEL = 248isize as _,
    MOUSE1 = 249isize as _,
    MOUSE2 = 250isize as _,
    GUI_FILTER = 251isize as _,
    GUI_RECENTER = 252isize as _,
    GUI_FOCUS = 253isize as _,
    GUI_USE_BUTTON = 254isize as _,
}
fn _Action_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], Action>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Digital state of an InputDeviceEffector (the m_State enum). IsSet/IsPressed are state in {2, 3}.
pub enum EffectorState {
    Idle = 0isize as _,
    Idle2 = 1isize as _,
    Clicked = 2isize as _,
    Pressed = 3isize as _,
    Released = 4isize as _,
    Frozen = 5isize as _,
}
fn _EffectorState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], EffectorState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// Maps action IDs to effector slots (255 action IDs total).
pub struct InputActionMap {}
impl InputActionMap {
    pub const GetActionEffector_ADDRESS: usize = 0x1402F43B0;
    pub unsafe fn GetActionEffector(
        &mut self,
        action_id: i32,
        device_index: i32,
    ) -> *mut crate::input::input_action_map::InputDeviceEffector {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                action_id: i32,
                device_index: i32,
            ) -> *mut crate::input::input_action_map::InputDeviceEffector = ::std::mem::transmute(
                Self::GetActionEffector_ADDRESS,
            );
            f(self as *mut Self as _, action_id, device_index)
        }
    }
}
impl std::convert::AsRef<InputActionMap> for InputActionMap {
    fn as_ref(&self) -> &InputActionMap {
        self
    }
}
impl std::convert::AsMut<InputActionMap> for InputActionMap {
    fn as_mut(&mut self) -> &mut InputActionMap {
        self
    }
}
#[repr(C, align(4))]
/// One input effector slot. Layout from the debug PDB (Input::InputDeviceEffector, 0x14),
/// cross-checked against retail usage (m_Value@0, m_State@8, m_StateTime@0x10 all match). The
/// pointer GetActionEffector returns is the head of a linked-list node whose extra id/next fields
/// follow this struct; reading the effector itself uses these offsets.
pub struct InputDeviceEffector {
    pub m_Value: f32,
    pub m_PrevValue: f32,
    pub m_State: crate::input::input_action_map::EffectorState,
    pub m_IsAnalogue: bool,
    pub m_IsDeltaBased: bool,
    pub m_IsUpdated: bool,
    pub m_ForceClick: bool,
    pub m_StateTime: f32,
}
fn _InputDeviceEffector_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14], InputDeviceEffector>([0u8; 0x14]);
    }
    unreachable!()
}
impl InputDeviceEffector {
    pub const Click_ADDRESS: usize = 0x1402EE630;
    /// Sets m_Value to 1.0 and m_State to Clicked (a one-frame press edge).
    pub unsafe fn Click(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Click_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Press_ADDRESS: usize = 0x1402EE660;
    /// Sets m_Value and m_State to Pressed/Held.
    pub unsafe fn Press(&mut self, value: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, value: f32) = ::std::mem::transmute(
                Self::Press_ADDRESS,
            );
            f(self as *mut Self as _, value)
        }
    }
    pub const Freeze_ADDRESS: usize = 0x1402EE6B0;
    /// Latches the effector into the Frozen state (ignores the device poll until cleared).
    pub unsafe fn Freeze(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Freeze_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ForceClick_ADDRESS: usize = 0x1402EE6D0;
    /// Sets m_ForceClick so the click survives the next per-frame poll (UpdateForceClicks consumes it).
    pub unsafe fn ForceClick(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ForceClick_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<InputDeviceEffector> for InputDeviceEffector {
    fn as_ref(&self) -> &InputDeviceEffector {
        self
    }
}
impl std::convert::AsMut<InputDeviceEffector> for InputDeviceEffector {
    fn as_mut(&mut self) -> &mut InputDeviceEffector {
        self
    }
}
#[repr(C, align(8))]
/// The local player's action map: a write-side wrapper that drives effectors by action ID. The
/// global at 0x142_F38_740 holds the instance pointer. Invalid action IDs resolve to a shared
/// null-effector sentinel, which the setters guard against. Action-name-to-ID is a static table
/// (action_name_table), so action IDs are stable across builds and can be referenced directly.
pub struct LocalPlayerActionMap {}
impl LocalPlayerActionMap {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418223424usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl LocalPlayerActionMap {
    pub const ForceSetPressed_ADDRESS: usize = 0x140C124B0;
    /// Drives an analog / held action: sets the effector's value (and presses it).
    pub unsafe fn ForceSetPressed(
        &mut self,
        action: crate::input::input_action_map::Action,
        value: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                action: crate::input::input_action_map::Action,
                value: f32,
            ) = ::std::mem::transmute(Self::ForceSetPressed_ADDRESS);
            f(self as *mut Self as _, action, value)
        }
    }
    pub const ForceSetClicked_ADDRESS: usize = 0x140C12480;
    /// Drives a one-frame click for an action (a press edge that survives the poll).
    pub unsafe fn ForceSetClicked(
        &mut self,
        action: crate::input::input_action_map::Action,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                action: crate::input::input_action_map::Action,
            ) = ::std::mem::transmute(Self::ForceSetClicked_ADDRESS);
            f(self as *mut Self as _, action)
        }
    }
}
impl std::convert::AsRef<LocalPlayerActionMap> for LocalPlayerActionMap {
    fn as_ref(&self) -> &LocalPlayerActionMap {
        self
    }
}
impl std::convert::AsMut<LocalPlayerActionMap> for LocalPlayerActionMap {
    fn as_mut(&mut self) -> &mut LocalPlayerActionMap {
        self
    }
}
pub unsafe fn get_action_name_table() -> &'static mut *const u8 {
    unsafe { &mut *(0x142D99370 as *mut *const u8) }
}
