#![allow(
    dead_code,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast,
    clippy::module_inception
)]
#![cfg_attr(any(), rustfmt::skip)]
pub mod aim;
pub mod animation;
pub mod audio;
pub mod camera;
pub mod character;
pub mod clock;
pub mod environment;
pub mod game;
pub mod graphics_engine;
pub mod hash;
pub mod input;
pub mod state;
pub mod types;
pub mod ui;
pub mod window;
