# Environment control: time of day and weather

Debug-UI controls to change the time of day and the weather in-game, for testing rendering under different lighting. All addresses below are live-verified in the release i64.

## Time of day

`CWorldTime::SetTimeOfDay` (`0x14052cd20`) is `void(this, float hours)`, hours 0–24 — it `fmod`s to 24, fires the per-hour event, and clamps against `PauseTimeOfDay`. This is the preferred setter. The singleton is `qword_142F17250` (a `CSingle<CWorldTime>::Instance` pointer); the current value is the float `CurrentTimeOfDay` at `+0x80` (`+0x84` is `PauseTimeOfDay`, `+0x88` is `SpeedMultiplicator`).

- Read: `*(f32*)(*(void**)0x142F17250 + 0x80)`
- Write: `SetTimeOfDay(*(void**)0x142F17250, hours)` — a raw write to `+0x80` works too, but skips the clamp and the hour event.

The render engine copies `CurrentTimeOfDay` out each frame, so writing the field propagates to lighting automatically. `CDayCycle::Apply` (`0x140495f30`) is the in-engine reference that reads and writes both this and the weather.

## Weather

Weather isn't a named-preset enum — it's a continuous model of scalars on the live `CWeather` object. Reach it through the landscape manager:

    landscapeMgr = *(void**)0x142F15648            (CLandscapeManager::Instance)
    atmosphere   = *(void**)(landscapeMgr + 0x120)
    weather      = *(void**)(atmosphere + 0x518)   (CAtmosphere::GetWeather 0x14033dc10 returns [this+0x518])

Direct writers: `CWeather::SetSeverity` (`0x1403a2290`) writes `m_Severity` at `+0x1E8` (~0.1 clear, ~4.0 full storm) and clears `m_DoUpdateSeverity` (`+0x20A`); `CWeather::UpdateSeverityTarget` (`0x1403a1f90`) does a smooth transition; `SetWeatherTime` (`0x1403a2250`). `m_SnowRatio` and `m_RainIntensity` sit at `+0x1C` and `+0x20` (from the dump's struct layout — high-confidence, but not instruction-verified).

The catch: `CWeatherController::Update` overwrites these toward its own targets each frame unless the controller's force flags are set. So the robust, engine-sanctioned way to *hold* a weather state is to fire the named weather events the controller listens for — `weather_sunny`, `weather_rain`, `weather_snow`, `weather_restore`, `weather_instant` (plus `cloud_base`, `cloud_height`). Send one via `SEventNameHash(name)` → `SEventID` → `NEvent::CSendEvent::SendMsg(1, &id, 0)`, the idiom in `NGSONodes::SetTimeOfDay` (`0x140a2c5e0`); `CWeatherController::Init` (`0x1403a24f0`) subscribes them. The events set: rain → severity 4.0, intensity 1.0, snow 0; snow → severity 4.0, snow 1.0; sunny → severity 0.1.

## UI

A 0–24h time slider writing `SetTimeOfDay`, and a weather dropdown firing the named events (simplest and robust), with an optional raw severity slider for fine control.
