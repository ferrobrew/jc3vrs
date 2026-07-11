// Foveation mask pixel shader (issue #29): tags a dithered fraction of the peripheral pixels in the
// stencil buffer so the expensive scene passes discard them. Runs full-screen (capture_vs) against the
// main depth-stencil surface with a stencil-write depth-stencil state (StencilOp REPLACE, ref = the mask
// bit); it writes no colour. A fragment that survives (is not discarded) here has the mask bit written to
// its stencil, marking it for the per-pass stencil test to drop; a discarded fragment keeps full
// resolution (its stencil bit stays clear).

cbuffer FoveationParams : register(b0)
{
    float2 g_CenterPx;   // foveal centre, in pixels (per-eye principal point)
    float  g_InnerPx;    // radius (px): inside = full resolution, no mask bit
    float  g_OuterPx;    // radius (px): the drop ramp reaches its maximum here
    float  g_MaxDrop;    // maximum fraction of peripheral pixels dropped (0..1)
    float3 _pad;
};

// Interleaved gradient noise (Jimenez 2014): a stable, screen-space dither in [0, 1).
float ign(float2 p)
{
    return frac(52.9829189 * frac(dot(p, float2(0.06711056, 0.00583715))));
}

void main(float4 pos : SV_Position)
{
    float d = distance(pos.xy, g_CenterPx);
    if (d <= g_InnerPx)
        discard;                                            // foveal centre: keep full resolution
    float ramp = saturate((d - g_InnerPx) / max(g_OuterPx - g_InnerPx, 1.0));
    float drop = ramp * g_MaxDrop;                          // fraction to drop at this radius
    if (ign(pos.xy) >= drop)
        discard;                                            // kept full resolution -> no mask bit
    // reaching here: this pixel is dropped -> the output-merger writes the mask bit (StencilOp REPLACE).
}
