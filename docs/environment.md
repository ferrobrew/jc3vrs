# Environment control: time of day and weather

Debug-UI controls to change the time of day and the weather in-game, for testing rendering under different lighting. The engine handles — addresses, offsets, and the caveats — live in the pyxis-defs (`WorldTime`, `Weather`, `Atmosphere`, `LandscapeManager`, `WeatherController`, `DayCycle`); this doc is the how and the gotchas.

## Time of day

`WorldTime::SetTimeOfDay` takes hours (0–24); it wraps to 24, fires the per-hour event, and clamps against `m_PauseTimeOfDay`. It's the preferred setter over writing `m_CurrentTimeOfDay` on the `WorldTime` singleton directly, which skips the clamp and the event. The render engine copies `m_CurrentTimeOfDay` out each frame, so the change propagates to lighting on its own. `DayCycle::Apply` is the in-engine reference that drives both time and weather.

## Weather

Weather isn't a named-preset enum — it's a continuous scalar model on the live `Weather`, reached through the `LandscapeManager` singleton → its `m_Atmosphere` → `Atmosphere::GetWeather` (the `m_Weather` it hands back). The direct writers are `Weather::SetSeverity` (~0.1 clear .. ~4.0 storm), `UpdateSeverityTarget` (smooth), and `SetWeatherTime`, plus the `m_SnowRatio` / `m_RainIntensity` scalars.

The catch is that `WeatherController` overwrites those scalars toward its own targets every frame, so a direct write won't hold. The robust, engine-sanctioned way to *pin* a weather state is to fire one of the named events the controller subscribes (in `WeatherController::Init`) — `weather_sunny`, `weather_rain`, `weather_snow`, `weather_restore`, `weather_instant`, plus `cloud_base` / `cloud_height` — via the engine's event send (`SEventNameHash` → `SEventID` → `NEvent::CSendEvent::SendMsg`); `NGSONodes::SetTimeOfDay` is a compact example of that idiom. The events set rain → severity 4.0 / intensity 1.0 / snow 0, snow → severity 4.0 / snow 1.0, sunny → severity 0.1.

## UI

A 0–24h time slider calling `SetTimeOfDay`, and a weather dropdown firing the named events (simplest and robust), with an optional raw severity slider for fine control.
