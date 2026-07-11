// Foveation fill-in pixel shader (issue #29): reconstructs the peripheral pixels the mask pass dropped.
// Runs full-screen (capture_vs) with the main colour buffer as the render target and a copy of it bound as
// the source; it re-derives, per pixel, whether the mask pass dropped it (identical radial + dither logic),
// passes through the kept pixels unchanged, and reconstructs each dropped pixel from the average of its
// non-dropped neighbours. Far in the periphery, where every neighbour was dropped, it falls back to the
// source value.

Texture2D SceneColor : register(t0);

cbuffer FoveationParams : register(b0)
{
    float2 g_CenterPx;   // foveal centre, in pixels (per-eye principal point)
    float  g_InnerPx;    // radius (px): inside = full resolution, no mask bit
    float  g_OuterPx;    // radius (px): the drop ramp reaches its maximum here
    float  g_MaxDrop;    // maximum fraction of peripheral pixels dropped (0..1)
    float  g_DebugMode;  // >0.5: tint dropped pixels magenta instead of reconstructing them
    float2 _pad;
};

// Interleaved gradient noise (Jimenez 2014): must match foveation_mask_ps.hlsl exactly.
float ign(float2 p)
{
    return frac(52.9829189 * frac(dot(p, float2(0.06711056, 0.00583715))));
}

// Reproduces the mask pass's per-pixel drop decision for the pixel centred at `px`.
bool is_dropped(float2 px)
{
    float d = distance(px, g_CenterPx);
    if (d <= g_InnerPx)
        return false;
    float ramp = saturate((d - g_InnerPx) / max(g_OuterPx - g_InnerPx, 1.0));
    return ign(px) < ramp * g_MaxDrop;
}

float4 main(float4 pos : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    int2 p = int2(pos.xy);
    if (!is_dropped(pos.xy))
        return SceneColor.Load(int3(p, 0));                 // kept full resolution: pass through

    // Diagnostic: paint the dropped set magenta so the mask is directly visible (no reconstruction).
    if (g_DebugMode > 0.5)
        return float4(1.0, 0.0, 1.0, 1.0);

    float3 sum = 0.0;
    float weight = 0.0;
    [unroll] for (int dy = -1; dy <= 1; dy++)
    {
        [unroll] for (int dx = -1; dx <= 1; dx++)
        {
            if (dx == 0 && dy == 0)
                continue;
            int2 n = p + int2(dx, dy);
            if (!is_dropped(float2(n) + 0.5))
            {
                sum += SceneColor.Load(int3(n, 0)).rgb;
                weight += 1.0;
            }
        }
    }
    if (weight > 0.0)
        return float4(sum / weight, 1.0);
    return SceneColor.Load(int3(p, 0));                     // deep periphery: no kept neighbour to sample
}
