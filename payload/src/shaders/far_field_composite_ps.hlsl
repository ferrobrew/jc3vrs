// Far-field G-buffer composite (issue #32): writes the far dispatch's captured G-buffer + depth
// into a near dispatch's cleared targets, after the engine's clears and Z prepass and before the
// geometry passes. Runs full-screen (capture_vs) with the four G-buffer targets and the main depth
// surface bound; the depth-stencil state is GREATER_EQUAL with depth write, so the merge with the
// near dispatch's Z-prepass depth happens in the fixed-function test: far content lands only where
// nothing nearer already claimed the pixel (reverse-Z: nearer = greater), and equal depths (the
// same far model prepassed by both dispatches) take the far G-buffer content.
//
// The UV mapping is the per-eye affine reprojection: the far image was rendered at eye 0's pose
// and projection, and with a shared camera centre and parallel eyes the eye-to-eye mapping reduces
// to a per-axis scale + offset in NDC derived from the two off-axis projections (the general
// canted-display case needs a full homography; not handled yet). Depth values transfer unchanged:
// both projections share the near/far reverse-Z mapping, so equal view depths encode equally.
// Pixels that map outside the far image (the coverage strip from the projection difference) are
// discarded, leaving the near dispatch's cleared far-plane depth (sky) there.

Texture2D GBuffer0 : register(t0);
Texture2D GBuffer1 : register(t1);
Texture2D GBuffer2 : register(t2);
Texture2D GBuffer3 : register(t3);
Texture2D<float2> FarDepth : register(t4); // R32_FLOAT_X8X24 (depth in .x)

cbuffer CompositeParams : register(b0)
{
    float2 g_NdcScale;  // per-axis NDC scale from the eye's projection into the far image's
    float2 g_NdcOffset; // per-axis NDC offset
};

struct VSOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

struct PSOut
{
    float4 gb0 : SV_Target0;
    float4 gb1 : SV_Target1;
    float4 gb2 : SV_Target2;
    float4 gb3 : SV_Target3;
    float depth : SV_Depth;
};

PSOut main(VSOut input)
{
    // Output UV -> output NDC -> far-image NDC -> far-image UV. NDC y is up, UV v is down.
    float2 ndc = float2(input.uv.x * 2.0 - 1.0, 1.0 - input.uv.y * 2.0);
    float2 far_ndc = ndc * g_NdcScale + g_NdcOffset;
    if (any(abs(far_ndc) > 1.0))
    {
        discard;
    }
    float2 far_uv = float2(far_ndc.x * 0.5 + 0.5, 0.5 - far_ndc.y * 0.5);

    uint w, h;
    GBuffer0.GetDimensions(w, h);
    int3 texel = int3(int2(far_uv * float2(w, h)), 0);

    float depth = FarDepth.Load(texel).x;
    if (depth <= 0.0)
    {
        // Far-plane clear: nothing was rendered here; keep the near dispatch's clear.
        discard;
    }

    PSOut output;
    output.gb0 = GBuffer0.Load(texel);
    output.gb1 = GBuffer1.Load(texel);
    output.gb2 = GBuffer2.Load(texel);
    output.gb3 = GBuffer3.Load(texel);
    output.depth = depth;
    return output;
}
