#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Atmosphere {
    _field_0: [u8; 1304],
    /// The live weather, also returned by [`GetWeather`](Atmosphere::GetWeather).
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
    /// The in-engine reference that reads and writes both the time of day and the weather each cycle:
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
/// The live weather state: a continuous scalar model.
///
/// **Caution:** [`WeatherController`]'s per-frame update overwrites these scalars toward its own
/// targets unless a force flag or a named weather event holds them, so prefer firing an event (see
/// [`WeatherController::Init`]) to pin a state.
pub struct Weather {
    _field_0: [u8; 28],
    /// The snow blend, in `0..1`.
    ///
    /// **Unverified:** from the dump's struct layout, not instruction-verified.
    pub m_SnowRatio: f32,
    /// The rain intensity, in `0..1`.
    ///
    /// **Unverified:** from the dump's struct layout, not instruction-verified.
    pub m_RainIntensity: f32,
    _field_24: [u8; 452],
    /// The storm severity, from roughly `0.1` (clear) to `4.0` (full storm).
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
    /// Sets `m_Severity` and clears the severity-update flag. See the type's caution about the
    /// controller overwriting it.
    pub unsafe fn SetSeverity(&mut self, severity: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, severity: f32) = ::std::mem::transmute(
                Self::SetSeverity_ADDRESS,
            );
            f(self as *mut Self as _, severity)
        }
    }
    pub const UpdateSeverityTarget_ADDRESS: usize = 0x1403A1F90;
    /// Smoothly drives the severity toward `target` over `update_time` seconds.
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
    /// Subscribes the named weather events (the `EVENT_*` constants, plus `cloud_base` and
    /// `cloud_height`). Firing one via the engine's event send is the robust way to hold a weather
    /// state, since the controller's per-frame update otherwise overwrites the [`Weather`]
    /// scalars.
    pub unsafe fn Init(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Init_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl WeatherController {
    /// Applies the pinned state instantly rather than blending toward it.
    pub const EVENT_INSTANT: &str = "weather_instant";
    /// Pins rain (severity `4.0`, rain intensity `1.0`, snow `0`).
    pub const EVENT_RAIN: &str = "weather_rain";
    /// Hands control back to the ambient weather system.
    pub const EVENT_RESTORE: &str = "weather_restore";
    /// Pins snow (severity `4.0`, snow ratio `1.0`).
    pub const EVENT_SNOW: &str = "weather_snow";
    /// Pins clear weather (severity `0.1`).
    pub const EVENT_SUNNY: &str = "weather_sunny";
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
    /// The current time of day, in hours. The render engine copies this out each frame, so a write
    /// here propagates to lighting.
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
    /// Sets the time of day in hours: wraps to a 24-hour range, fires the per-hour event, and clamps
    /// against `m_PauseTimeOfDay`. Preferred over writing `m_CurrentTimeOfDay` directly.
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
