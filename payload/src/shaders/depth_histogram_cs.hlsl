// Depth-distribution histogram for the dynamic HUD distance (issue #14).
//
// Samples the main scene depth buffer on a stride grid, linearizes each sample through the
// projection's z-row (works for standard and reversed-Z alike), and accumulates a log-spaced,
// fixed-point-weighted histogram over [BIN_MIN_METERS, BIN_MAX_METERS]. Slot BIN_COUNT holds the
// total weight.
//
// When MaskByHud is set, each sample is weighted by the HUD texture's alpha where the sample's
// camera ray meets the floating panel, so the statistics describe "the depth of the scene behind
// visible HUD content" rather than the whole frame -- a first-person weapon filling the view no
// longer reads as near-field unless it actually sits behind HUD elements. Samples nearer than
// MinDepth are ignored outright (a floor for viewmodel geometry).
//
// The consumer clears the histogram buffer before dispatch and reads it back asynchronously.

#define BIN_COUNT 32
#define BIN_MIN_METERS 0.25
#define BIN_MAX_METERS 256.0
// Fixed-point scale for alpha weights (1.0 => 256).
#define WEIGHT_ONE 256

cbuffer Params : register(b0)
{
    // Depth texture dimensions in pixels.
    float2 DepthDims;
    // Projection z-row terms: device depth d = ProjA + ProjB / view_z, so
    // view_z = ProjB / (d - ProjA).
    float ProjA;
    float ProjB;
    // Sample every Nth pixel on both axes; whether to weight by the HUD panel's alpha; samples
    // nearer than MinDepth are discarded.
    uint Stride;
    uint MaskByHud;
    float MinDepth;
    float _pad0;
    // Camera origin and the panel plane: origin is the panel's top-left corner; RightAxis and
    // UpAxis span it, pre-divided by the squared extents so a dot product yields UV directly.
    float4 CameraPos;
    float4 PanelOrigin;
    float4 PanelRight;
    float4 PanelUp;
    // The inverse view-projection, for unprojecting depth pixels to world-space rays.
    float4x4 InvViewProjection;
}

Texture2D<float> Depth : register(t0);
Texture2D<float4> Hud : register(t1);
SamplerState HudSampler : register(s0);
RWBuffer<uint> Histogram : register(u0);

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    const uint2 pixel = id.xy * Stride;
    if (pixel.x >= (uint) DepthDims.x || pixel.y >= (uint) DepthDims.y)
    {
        return;
    }

    uint weight = WEIGHT_ONE;
    if (MaskByHud != 0)
    {
        // Unproject the pixel to a world-space ray and intersect the panel plane.
        const float2 ndc = float2(
            ((float) pixel.x + 0.5) / DepthDims.x * 2.0 - 1.0,
            1.0 - ((float) pixel.y + 0.5) / DepthDims.y * 2.0);
        float4 far_h = mul(float4(ndc, 1.0, 1.0), InvViewProjection);
        const float3 dir = far_h.xyz / far_h.w - CameraPos.xyz;
        const float3 normal = cross(PanelRight.xyz, PanelUp.xyz);
        const float denom = dot(dir, normal);
        if (abs(denom) < 1e-6)
        {
            return;
        }
        const float t = dot(PanelOrigin.xyz - CameraPos.xyz, normal) / denom;
        if (t <= 0.0)
        {
            return;
        }
        const float3 local = CameraPos.xyz + dir * t - PanelOrigin.xyz;
        // PanelRight/PanelUp carry 1/extent^2, so these dots are UVs in [0, 1] on the panel.
        const float2 uv = float2(dot(local, PanelRight.xyz) * PanelRight.w,
                                 dot(local, PanelUp.xyz) * PanelUp.w);
        if (any(uv < 0.0) || any(uv > 1.0))
        {
            return;
        }
        const float alpha = Hud.SampleLevel(HudSampler, uv, 0).a;
        weight = (uint) (saturate(alpha) * WEIGHT_ONE);
        if (weight == 0)
        {
            return;
        }
    }

    const float device_depth = Depth[pixel];
    float view_z = ProjB / (device_depth - ProjA);
    // Guard degenerate projections and the clear value (view_z can come out negative,
    // infinite, or NaN there); such samples count toward the total as far-field.
    if (!(view_z > 0.0))
    {
        view_z = BIN_MAX_METERS;
    }
    if (view_z < MinDepth)
    {
        return;
    }

    const float t_bin = log2(max(view_z, BIN_MIN_METERS) / BIN_MIN_METERS)
        / log2(BIN_MAX_METERS / BIN_MIN_METERS);
    const uint bin = min((uint) (saturate(t_bin) * BIN_COUNT), BIN_COUNT - 1);

    uint ignored;
    InterlockedAdd(Histogram[bin], weight, ignored);
    InterlockedAdd(Histogram[BIN_COUNT], weight, ignored);
}
