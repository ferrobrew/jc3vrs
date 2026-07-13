#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// An animation rule system (`CAnimationRuleSystem`): drives one afsmb state machine for a character.
/// A character's [`AnimatedModel`](crate::character::character::AnimatedModel) holds one per animated model,
/// the first being the body's.
pub struct AnimationRuleSystem {
    _field_0: [u8; 72],
    /// The running state machine instance this rule system drives.
    pub m_StateMachineInstance: crate::types::shared_ptr::SharedPtr<
        crate::animation::rule_system::StateMachineInstance,
    >,
}
fn _AnimationRuleSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], AnimationRuleSystem>([0u8; 0x58]);
    }
    unreachable!()
}
impl AnimationRuleSystem {}
impl std::convert::AsRef<AnimationRuleSystem> for AnimationRuleSystem {
    fn as_ref(&self) -> &AnimationRuleSystem {
        self
    }
}
impl std::convert::AsMut<AnimationRuleSystem> for AnimationRuleSystem {
    fn as_mut(&mut self) -> &mut AnimationRuleSystem {
        self
    }
}
#[repr(C, align(8))]
/// An animation state (`NAnimationSystem::CState`): one node of a character's animation state machine,
/// i.e. an afsmb `S_*` rule state. Identified by its name hash.
pub struct State {
    _field_0: [u8; 16],
    /// The state's name hash (`hashlittle(state_name)`, e.g. `hashlittle("S_IDLE")`), identifying
    /// which afsmb state this is. Read by `CCharacter::IsInVehicleAttachState` and the other
    /// current-state checks.
    pub m_HashID: crate::hash::HashString,
    _field_14: [u8; 4],
}
fn _State_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], State>([0u8; 0x18]);
    }
    unreachable!()
}
impl State {}
impl std::convert::AsRef<State> for State {
    fn as_ref(&self) -> &State {
        self
    }
}
impl std::convert::AsMut<State> for State {
    fn as_mut(&mut self) -> &mut State {
        self
    }
}
#[repr(C, align(8))]
/// A running animation state machine (`NAnimationSystem::CStateMachineInstance`): tracks the current
/// [`State`] for one [`AnimationRuleSystem`].
pub struct StateMachineInstance {
    _field_0: [u8; 24],
    /// The state the machine is currently in.
    pub m_CurrentState: *mut crate::animation::rule_system::State,
}
fn _StateMachineInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], StateMachineInstance>([0u8; 0x20]);
    }
    unreachable!()
}
impl StateMachineInstance {}
impl std::convert::AsRef<StateMachineInstance> for StateMachineInstance {
    fn as_ref(&self) -> &StateMachineInstance {
        self
    }
}
impl std::convert::AsMut<StateMachineInstance> for StateMachineInstance {
    fn as_mut(&mut self) -> &mut StateMachineInstance {
        self
    }
}
