//! Dynamic HUD distance from the scene depth distribution (issue #14).
//!
//! A compute pass ([`depth_histogram_cs`](../shaders/depth_histogram_cs.hlsl)) samples the whole
//! main depth buffer on a stride grid and accumulates a log-spaced histogram of linear view
//! depths. The result reads back asynchronously (a small staging ring, never stalling the GPU),
//! and a policy drives the floating panel's distance from it: when enough of the frame sits in
//! the near field (a vehicle interior, a corridor, a wall), the panel eases toward the near
//! distance so it does not interpenetrate the scene; otherwise it eases back to the configured
//! base distance. Full-screen UI (menus, movies) always reads as far. The panel's
//! constant-apparent-size scaling makes the shift a pure depth change.
//!
//! The default policy is threshold occupancy with hysteresis; a continuous percentile-following
//! policy is available behind [`DepthShiftConfig::continuous`] for experimentation.

use anyhow::Context as _;
use jc3gi::graphics_engine::{device::Device, graphics_engine::GraphicsEngine};
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC,
        D3D11_BUFFER_UAV, D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE,
        D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAP_WRITE_DISCARD,
        D3D11_MAPPED_SUBRESOURCE, D3D11_UAV_DIMENSION_BUFFER, D3D11_UNORDERED_ACCESS_VIEW_DESC,
        D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DYNAMIC, D3D11_USAGE_STAGING, ID3D11Buffer,
        ID3D11ComputeShader, ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11RenderTargetView,
        ID3D11UnorderedAccessView,
    },
    Dxgi::Common::DXGI_FORMAT_R32_UINT,
};

use super::{HudMode, config::DepthShiftConfig};

/// The committed, precompiled histogram compute shader (entry point `main`).
const COMPUTE_DXBC: &[u8] = include_bytes!("../shaders/depth_histogram_cs.dxbc");

/// Histogram layout, matching the HLSL: `BIN_COUNT` log-spaced bins over
/// [`BIN_MIN_METERS`, `BIN_MAX_METERS`], plus the total sample count in the last slot.
const BIN_COUNT: usize = 32;
const BIN_MIN_METERS: f32 = 0.25;
const BIN_MAX_METERS: f32 = 256.0;
const SLOT_COUNT: usize = BIN_COUNT + 1;

/// Staging-ring depth: reads happen this many frames behind the dispatch, which the smoothing
/// makes irrelevant.
const RING_DEPTH: usize = 3;

/// Constant buffer, matching the HLSL `Params`.
#[repr(C)]
struct Params {
    depth_dims: [f32; 2],
    proj_a: f32,
    proj_b: f32,
    stride: u32,
    mask_by_hud: u32,
    min_depth: f32,
    _pad0: f32,
    camera_pos: [f32; 4],
    panel_origin: [f32; 4],
    panel_right: [f32; 4],
    panel_up: [f32; 4],
    inv_view_projection: [f32; 16],
}

/// The fixed-point weight the shader uses for an unmasked sample (matches `WEIGHT_ONE`).
const WEIGHT_ONE: u32 = 256;

/// The panel-mask inputs for one dispatch: the ray origin, the panel's world corners (top-left,
/// top-right, bottom-left as the HUD texture maps), and the HUD texture to weight by.
pub struct MaskInputs<'a> {
    pub camera_pos: [f32; 3],
    pub corners: [[f32; 4]; 4],
    pub hud_srv: &'a windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
}

/// Whether the local player is attached to a vehicle, polled on the game thread (the accessor
/// walks game-thread animation state) and read by the render-side policy.
static IN_VEHICLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Poll the local player's vehicle-attach state. Call once per frame on the game update thread.
pub fn poll_vehicle_state() {
    // SAFETY: game update thread; the accessor reads the character's own animation state.
    let in_vehicle = unsafe {
        jc3gi::character::character::Character::GetLocalPlayerCharacter()
            .as_ref()
            .is_some_and(|c| c.IsInVehicleAttachState())
    };
    IN_VEHICLE.store(in_vehicle, std::sync::atomic::Ordering::Relaxed);
}

/// A live snapshot of the dynamic-distance state, for the debug UI.
#[derive(Clone, Copy)]
pub struct DepthShiftStatus {
    /// The latest read-back statistics, if any.
    pub stats: Option<DepthStats>,
    /// The smoothed panel distance currently applied.
    pub smoothed: Option<f32>,
    /// Whether the near shift is engaged (threshold policy).
    pub near_engaged: bool,
}

/// The frame's depth-distribution statistics, derived from a read-back histogram.
#[derive(Clone, Copy, Debug)]
pub struct DepthStats {
    /// The approximate sample count (the shader accumulates fixed-point alpha weights; this is
    /// the total weight normalized back to whole samples).
    pub total: u32,
    /// Fraction of samples nearer than the configured near threshold.
    pub near_occupancy: f32,
    /// The depth below which the configured percentile of samples lies.
    pub percentile_depth: f32,
}

/// The dynamic-distance state: the compute pipeline, the readback ring, and the policy's
/// smoothing state.
pub struct DepthShift {
    shader: ID3D11ComputeShader,
    sampler: windows::Win32::Graphics::Direct3D11::ID3D11SamplerState,
    params: ID3D11Buffer,
    histogram: ID3D11Buffer,
    histogram_uav: ID3D11UnorderedAccessView,
    staging: [ID3D11Buffer; RING_DEPTH],
    /// Which staging slots hold an in-flight copy.
    in_flight: [bool; RING_DEPTH],
    frame: usize,
    /// The latest read-back statistics.
    stats: Option<DepthStats>,
    /// Whether the near shift is currently engaged (the hysteresis state).
    near_engaged: bool,
    /// The smoothed panel distance; starts at the first target it sees.
    smoothed: Option<f32>,
    last_update: Option<std::time::Instant>,
    last_log: Option<std::time::Instant>,
}

impl DepthShift {
    /// Build the compute pipeline and the readback ring.
    pub fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors below are valid for these
        // calls.
        unsafe {
            let mut shader: Option<ID3D11ComputeShader> = None;
            d3d.CreateComputeShader(COMPUTE_DXBC, None, Some(&mut shader))
                .context("creating the depth-histogram compute shader")?;
            let shader = shader.context("the depth-histogram compute shader was not created")?;

            let mut sampler = None;
            d3d.CreateSamplerState(
                &windows::Win32::Graphics::Direct3D11::D3D11_SAMPLER_DESC {
                    Filter: windows::Win32::Graphics::Direct3D11::D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )
            .context("creating the depth-histogram HUD sampler")?;
            let sampler = sampler.context("the depth-histogram HUD sampler was not created")?;

            let mut params: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<Params>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut params),
            )
            .context("creating the depth-histogram constant buffer")?;
            let params = params.context("the depth-histogram constant buffer was not created")?;

            let mut histogram: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: (SLOT_COUNT * 4) as u32,
                    BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut histogram),
            )
            .context("creating the depth-histogram buffer")?;
            let histogram = histogram.context("the depth-histogram buffer was not created")?;

            let mut histogram_uav: Option<ID3D11UnorderedAccessView> = None;
            d3d.CreateUnorderedAccessView(
                &histogram,
                Some(&D3D11_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_UINT,
                    ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                        Buffer: D3D11_BUFFER_UAV {
                            FirstElement: 0,
                            NumElements: SLOT_COUNT as u32,
                            Flags: 0,
                        },
                    },
                }),
                Some(&mut histogram_uav),
            )
            .context("creating the depth-histogram UAV")?;
            let histogram_uav = histogram_uav.context("the depth-histogram UAV was not created")?;

            let staging = std::array::from_fn::<_, RING_DEPTH, _>(|_| {
                let mut buffer: Option<ID3D11Buffer> = None;
                d3d.CreateBuffer(
                    &D3D11_BUFFER_DESC {
                        ByteWidth: (SLOT_COUNT * 4) as u32,
                        Usage: D3D11_USAGE_STAGING,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut buffer),
                )
                .ok()
                .and(buffer)
            });
            let staging: [_; RING_DEPTH] = staging
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .context("creating the depth-histogram staging ring")?
                .try_into()
                .unwrap();

            Ok(Self {
                shader,
                sampler,
                params,
                histogram,
                histogram_uav,
                staging,
                in_flight: [false; RING_DEPTH],
                frame: 0,
                stats: None,
                near_engaged: false,
                smoothed: None,
                last_update: None,
                last_log: None,
            })
        }
    }

    /// Run the frame's histogram dispatch and pick up any completed readback. Call once per
    /// frame (eye 0) with the engine context mutex held.
    pub fn sample(
        &mut self,
        context: &ID3D11DeviceContext,
        graphics_engine: &GraphicsEngine,
        cfg: &DepthShiftConfig,
        mask: Option<MaskInputs<'_>>,
    ) {
        // SAFETY: the depth texture and its SRV belong to the live graphics engine; the buffers
        // are ours; the context is the engine's immediate context under its mutex.
        unsafe {
            let Some(depth) = graphics_engine.m_MainDepthTexture.as_ref() else {
                return;
            };
            let (width, height) = (f32::from(depth.m_Width), f32::from(depth.m_Height));
            if width < 1.0 || height < 1.0 {
                return;
            }
            let Some((proj_a, proj_b)) = projection_z_row() else {
                return;
            };

            // The panel-plane mapping for the HUD-alpha mask: the top-left corner as origin,
            // the u and v spans pre-divided by their squared lengths so the shader's dot
            // products yield UVs directly, and the inverse view-projection for the pixel rays.
            let (mask_by_hud, camera_pos, origin, right, up) = match &mask {
                Some(inputs) => {
                    let c = |i: usize| glam::Vec3::from_slice(&inputs.corners[i][..3]);
                    let (tl, tr, bl) = (c(0), c(1), c(2));
                    let (u_span, v_span) = (tr - tl, bl - tl);
                    let pack = |v: glam::Vec3| {
                        let len_sq = v.length_squared().max(f32::EPSILON);
                        [v.x, v.y, v.z, 1.0 / len_sq]
                    };
                    (
                        1u32,
                        [
                            inputs.camera_pos[0],
                            inputs.camera_pos[1],
                            inputs.camera_pos[2],
                            0.0,
                        ],
                        [tl.x, tl.y, tl.z, 0.0],
                        pack(u_span),
                        pack(v_span),
                    )
                }
                None => (0u32, [0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]),
            };
            let inv_view_projection = match super::quad::fetch_view_projection() {
                Some(vp) if mask_by_hud != 0 => {
                    // Invert in f64: the view-projection carries world-scale translations that
                    // lose precision in f32 (see the stereo reprojection).
                    let engine = jc3gi::types::math::Matrix4 { data: vp };
                    let inv = glam::Mat4::from(engine).as_dmat4().inverse().as_mat4();
                    jc3gi::types::math::Matrix4::from(inv).data
                }
                _ => [0.0; 16],
            };

            // Upload the frame's parameters.
            let stride = cfg.sample_stride.max(1);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context
                .Map(
                    &self.params,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .is_err()
            {
                return;
            }
            (mapped.pData as *mut Params).write(Params {
                depth_dims: [width, height],
                proj_a,
                proj_b,
                stride,
                mask_by_hud,
                min_depth: cfg.min_depth.max(0.0),
                _pad0: 0.0,
                camera_pos,
                panel_origin: origin,
                panel_right: right,
                panel_up: up,
                inv_view_projection,
            });
            context.Unmap(&self.params, 0);

            // The depth texture is typically still bound as the depth target here, and D3D
            // silently refuses a conflicting SRV bind (the shader then reads zeros and every
            // sample lands in the far bin). Detach the output merger for the dispatch and
            // restore it after.
            let mut saved_rtvs: [Option<ID3D11RenderTargetView>; 8] = Default::default();
            let mut saved_dsv: Option<ID3D11DepthStencilView> = None;
            context.OMGetRenderTargets(Some(&mut saved_rtvs), Some(&mut saved_dsv));
            context.OMSetRenderTargets(None, None);

            // Clear, bind, dispatch, unbind.
            context.ClearUnorderedAccessViewUint(&self.histogram_uav, &[0u32; 4]);
            context.CSSetShader(&self.shader, None);
            context.CSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            context.CSSetShaderResources(
                0,
                Some(&[
                    Some(depth.m_SRV.clone()),
                    mask.as_ref().map(|inputs| inputs.hud_srv.clone()),
                ]),
            );
            context.CSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.histogram_uav.clone())), None);
            let groups_x = ((width / stride as f32).ceil() as u32).div_ceil(8).max(1);
            let groups_y = ((height / stride as f32).ceil() as u32).div_ceil(8).max(1);
            context.Dispatch(groups_x, groups_y, 1);
            context.CSSetShaderResources(0, Some(&[None, None]));
            context.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
            context.CSSetShader(None, None);
            context.OMSetRenderTargets(Some(&saved_rtvs), saved_dsv.as_ref());

            // Queue this frame's copy and try to pick up the oldest in-flight one without
            // waiting.
            let slot = self.frame % RING_DEPTH;
            context.CopyResource(&self.staging[slot], &self.histogram);
            self.in_flight[slot] = true;
            self.frame += 1;

            let read_slot = self.frame % RING_DEPTH;
            if self.in_flight[read_slot] {
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                if context
                    .Map(
                        &self.staging[read_slot],
                        0,
                        D3D11_MAP_READ,
                        D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                        Some(&mut mapped),
                    )
                    .is_ok()
                {
                    let mut bins = [0u32; SLOT_COUNT];
                    (mapped.pData as *const u32).copy_to(bins.as_mut_ptr(), SLOT_COUNT);
                    context.Unmap(&self.staging[read_slot], 0);
                    self.in_flight[read_slot] = false;
                    self.stats = derive_stats(&bins, cfg);
                }
            }
        }
    }

    /// The frame's panel distance: the policy target eased with a critically-damped exponential
    /// (`halflife` seconds). Falls back to `base` (the configured panel distance) until stats
    /// exist. Call once per frame (eye 0), after [`sample`](DepthShift::sample).
    pub fn distance(&mut self, cfg: &DepthShiftConfig, mode: HudMode, base: f32) -> f32 {
        let target = self.target(cfg, mode, base);
        let dt = self
            .last_update
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.016)
            .min(0.1);
        self.last_update = Some(std::time::Instant::now());
        let alpha = (1.0 - 2.0_f32.powf(-dt / cfg.halflife.max(0.01))).min(1.0);
        let mut smoothed = self.smoothed.unwrap_or(target);
        smoothed += (target - smoothed) * alpha;
        self.smoothed = Some(smoothed);

        self.log_periodically(target, smoothed);
        smoothed
    }

    /// The policy's target distance for this frame, before smoothing.
    fn target(&mut self, cfg: &DepthShiftConfig, mode: HudMode, base: f32) -> f32 {
        // Full-screen UI reads as far, always.
        if mode != HudMode::Hud {
            self.near_engaged = false;
            return base;
        }
        if !cfg.use_depth_histogram {
            // Deterministic vehicle policy: near while attached to a vehicle. No hysteresis
            // needed -- the flag does not flap -- and the easing covers mount/dismount.
            self.near_engaged = IN_VEHICLE.load(std::sync::atomic::Ordering::Relaxed);
            return if self.near_engaged {
                cfg.near_distance
            } else {
                base
            };
        }
        let Some(stats) = self.stats else {
            return base;
        };
        if cfg.continuous {
            // Continuous: sit just inside the configured percentile of the scene.
            return (stats.percentile_depth - cfg.margin).clamp(cfg.near_distance, base);
        }
        // Threshold occupancy with hysteresis: engage the near shift when enough of the frame
        // is nearer than the threshold, release only once it drops clearly below.
        let engage = cfg.near_occupancy;
        let release = (cfg.near_occupancy - cfg.hysteresis).max(0.0);
        if self.near_engaged {
            if stats.near_occupancy < release {
                self.near_engaged = false;
            }
        } else if stats.near_occupancy >= engage {
            self.near_engaged = true;
        }
        if self.near_engaged {
            cfg.near_distance
        } else {
            base
        }
    }

    /// A snapshot of the live state for the debug UI.
    pub fn status(&self) -> DepthShiftStatus {
        DepthShiftStatus {
            stats: self.stats,
            smoothed: self.smoothed,
            near_engaged: self.near_engaged,
        }
    }

    /// Log the live statistics every few seconds while enabled, for tuning.
    fn log_periodically(&mut self, target: f32, smoothed: f32) {
        let due = self
            .last_log
            .is_none_or(|t| t.elapsed().as_secs_f32() >= 5.0);
        if !due {
            return;
        }
        self.last_log = Some(std::time::Instant::now());
        if let Some(stats) = self.stats {
            tracing::info!(
                "hud depth: near {:.0}% p{:.2}m -> target {target:.2}m (smoothed {smoothed:.2}m, \
                 {} samples)",
                stats.near_occupancy * 100.0,
                stats.percentile_depth,
                stats.total,
            );
        }
    }
}

/// Derive the policy inputs from a read-back histogram.
fn derive_stats(bins: &[u32; SLOT_COUNT], cfg: &DepthShiftConfig) -> Option<DepthStats> {
    let total = bins[BIN_COUNT];
    if total == 0 {
        return None;
    }
    let total_samples = total / WEIGHT_ONE;
    // Near occupancy: bins whose upper edge is below the threshold, plus a linear share of the
    // bin containing it.
    let mut near = 0.0f32;
    for (index, &count) in bins[..BIN_COUNT].iter().enumerate() {
        let (lo, hi) = bin_edges(index);
        if hi <= cfg.near_threshold {
            near += count as f32;
        } else if lo < cfg.near_threshold {
            near += count as f32 * ((cfg.near_threshold - lo) / (hi - lo)).clamp(0.0, 1.0);
        }
    }
    // Percentile: the upper edge of the bin where the cumulative fraction crosses it.
    let want = cfg.percentile.clamp(0.0, 1.0) * total as f32;
    let mut cumulative = 0.0f32;
    let mut percentile_depth = BIN_MAX_METERS;
    for (index, &count) in bins[..BIN_COUNT].iter().enumerate() {
        cumulative += count as f32;
        if cumulative >= want {
            percentile_depth = bin_edges(index).1;
            break;
        }
    }
    Some(DepthStats {
        total: total_samples,
        near_occupancy: near / total as f32,
        percentile_depth,
    })
}

/// The log-spaced `[lower, upper)` depth edges of a bin, in meters.
fn bin_edges(index: usize) -> (f32, f32) {
    let ratio = BIN_MAX_METERS / BIN_MIN_METERS;
    let lo = BIN_MIN_METERS * ratio.powf(index as f32 / BIN_COUNT as f32);
    let hi = BIN_MIN_METERS * ratio.powf((index + 1) as f32 / BIN_COUNT as f32);
    (lo, hi)
}

/// The render camera projection's z-row `(A, B)` with `device_depth = A + B / view_z`. The
/// embedded render camera is used (not the nullable active-camera pointer), matching the camera
/// the depth buffer was rendered with.
fn projection_z_row() -> Option<(f32, f32)> {
    // SAFETY: reads the live camera manager singleton, like render_camera_pose.
    let projection = unsafe {
        let cm = jc3gi::camera::camera_manager::CameraManager::get()?;
        cm.m_RenderCamera.as_ref()?.m_ProjectionF
    };
    let proj = glam::Mat4::from(projection);
    Some((proj.z_axis.z, proj.w_axis.z))
}
