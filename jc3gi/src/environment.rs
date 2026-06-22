#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Atmosphere {
    _field_0: [u8; 1304],
    /// The live Weather (also returned by GetWeather).
    pub m_Weather: *mut crate::environment::Weather,
}
fn _Atmosphere_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x520], Atmosphere>([0u8; 0x520]);
    }
    unreachable!()
}
impl Atmosphere {
    pub const GetWeather_ADDRESS: usize = 0x14033DC10;
    /// Returns m_Weather.
    pub unsafe fn GetWeather(&mut self) -> *mut crate::environment::Weather {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::environment::Weather = ::std::mem::transmute(
                Self::GetWeather_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<Atmosphere> for Atmosphere {
    fn as_ref(&self) -> &Atmosphere {
        self
    }
}
impl std::convert::AsMut<Atmosphere> for Atmosphere {
    fn as_mut(&mut self) -> &mut Atmosphere {
        self
    }
}
#[repr(C, align(8))]
pub struct DayCycle {}
impl DayCycle {
    pub const Apply_ADDRESS: usize = 0x140495F30;
    /// In-engine reference that reads and writes both the time-of-day and the weather each cycle --
    /// the canonical example of driving both systems.
    pub unsafe fn Apply(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Apply_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<DayCycle> for DayCycle {
    fn as_ref(&self) -> &DayCycle {
        self
    }
}
impl std::convert::AsMut<DayCycle> for DayCycle {
    fn as_mut(&mut self) -> &mut DayCycle {
        self
    }
}
#[repr(C, align(8))]
pub struct LandscapeManager {
    _field_0: [u8; 288],
    /// Reach the weather via Atmosphere::GetWeather / m_Weather.
    pub m_Atmosphere: *mut crate::environment::Atmosphere,
}
fn _LandscapeManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x128], LandscapeManager>([0u8; 0x128]);
    }
    unreachable!()
}
impl LandscapeManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418079816usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl LandscapeManager {}
impl std::convert::AsRef<LandscapeManager> for LandscapeManager {
    fn as_ref(&self) -> &LandscapeManager {
        self
    }
}
impl std::convert::AsMut<LandscapeManager> for LandscapeManager {
    fn as_mut(&mut self) -> &mut LandscapeManager {
        self
    }
}
#[repr(C, align(8))]
pub struct Weather {
    _field_0: [u8; 28],
    /// 0..1 snow blend. From the dump's struct layout; not instruction-verified.
    pub m_SnowRatio: f32,
    /// 0..1 rain intensity. From the dump's struct layout; not instruction-verified.
    pub m_RainIntensity: f32,
    _field_24: [u8; 452],
    /// Storm severity, ~0.1 clear .. ~4.0 full storm.
    pub m_Severity: f32,
    _field_1ec: [u8; 4],
}
fn _Weather_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1F0], Weather>([0u8; 0x1F0]);
    }
    unreachable!()
}
impl Weather {
    pub const SetSeverity_ADDRESS: usize = 0x1403A2290;
    /// Set m_Severity (~0.1 clear .. ~4.0 storm) and clear m_DoUpdateSeverity. NOTE:
    /// WeatherController's update overwrites m_Severity toward its own target unless a force flag
    /// or a named weather event holds it -- fire an event (see WeatherController) to pin a state.
    pub unsafe fn SetSeverity(&mut self, severity: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, severity: f32) = ::std::mem::transmute(
                Self::SetSeverity_ADDRESS,
            );
            f(self as *mut Self as _, severity)
        }
    }
    pub const UpdateSeverityTarget_ADDRESS: usize = 0x1403A1F90;
    /// Smoothly drive severity toward `target` over `update_time` seconds.
    pub unsafe fn UpdateSeverityTarget(&mut self, update_time: f32, target: f32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                update_time: f32,
                target: f32,
            ) = ::std::mem::transmute(Self::UpdateSeverityTarget_ADDRESS);
            f(self as *mut Self as _, update_time, target)
        }
    }
    pub const SetWeatherTime_ADDRESS: usize = 0x1403A2250;
    pub unsafe fn SetWeatherTime(&mut self, time: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, time: f32) = ::std::mem::transmute(
                Self::SetWeatherTime_ADDRESS,
            );
            f(self as *mut Self as _, time)
        }
    }
    pub const GetWeatherTime_ADDRESS: usize = 0x1403A2240;
    pub unsafe fn GetWeatherTime(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetWeatherTime_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<Weather> for Weather {
    fn as_ref(&self) -> &Weather {
        self
    }
}
impl std::convert::AsMut<Weather> for Weather {
    fn as_mut(&mut self) -> &mut Weather {
        self
    }
}
#[repr(C, align(8))]
pub struct WeatherController {}
impl WeatherController {
    pub const Init_ADDRESS: usize = 0x1403A24F0;
    /// Subscribes the named weather events: weather_sunny / weather_rain / weather_snow /
    /// weather_restore / weather_instant, plus cloud_base / cloud_height. Firing one (via the
    /// engine's event send) is the robust way to hold a weather state, since the controller's
    /// per-frame update otherwise overwrites the Weather scalars. Event severities: rain ->
    /// severity 4.0 / intensity 1.0 / snow 0; snow -> severity 4.0 / snow 1.0; sunny -> severity 0.1.
    pub unsafe fn Init(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Init_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<WeatherController> for WeatherController {
    fn as_ref(&self) -> &WeatherController {
        self
    }
}
impl std::convert::AsMut<WeatherController> for WeatherController {
    fn as_mut(&mut self) -> &mut WeatherController {
        self
    }
}
#[repr(C, align(8))]
pub struct WorldTime {
    _field_0: [u8; 128],
    /// Current time of day in hours (0-24). The render engine copies this out each frame, so a
    /// write here propagates to lighting.
    pub m_CurrentTimeOfDay: f32,
    pub m_PauseTimeOfDay: f32,
    pub m_SpeedMultiplicator: f32,
    _field_8c: [u8; 4],
}
fn _WorldTime_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], WorldTime>([0u8; 0x90]);
    }
    unreachable!()
}
impl WorldTime {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418086992usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl WorldTime {
    pub const SetTimeOfDay_ADDRESS: usize = 0x14052CD20;
    /// Set the time of day in hours (0-24): fmods to 24, fires the per-hour event, clamps against
    /// m_PauseTimeOfDay. Preferred over writing m_CurrentTimeOfDay directly.
    pub unsafe fn SetTimeOfDay(&mut self, hours: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, hours: f32) = ::std::mem::transmute(
                Self::SetTimeOfDay_ADDRESS,
            );
            f(self as *mut Self as _, hours)
        }
    }
}
impl std::convert::AsRef<WorldTime> for WorldTime {
    fn as_ref(&self) -> &WorldTime {
        self
    }
}
impl std::convert::AsMut<WorldTime> for WorldTime {
    fn as_mut(&mut self) -> &mut WorldTime {
        self
    }
}
