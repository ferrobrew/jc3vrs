// VR eye-blit pixel shader: samples the game's captured eye back buffer and writes it into one
// array slice of the OpenXR stereo swapchain. Paired with the fullscreen-triangle vertex shader
// (capture_vs), which the blit reuses.
//
// The captured eye texture is a copy of the game's display-referred (sRGB-encoded) back buffer,
// stored as R8G8B8A8_UNORM. The OpenXR swapchain is negotiated as _SRGB, so writing through its
// render-target view applies a hardware linear->sRGB encode. To reproduce the original bytes the
// shader linearizes the sampled colour first (g_linearize != 0), so the hardware re-encode cancels
// it out. g_linearize == 0 passes the colour through unchanged (for a genuine-linear source or a
// non-sRGB target). See vr::config::BlitGamma for the reasoning and the runtime toggle.

cbuffer BlitParams : register(b0)
{
    uint g_linearize;
    uint3 _pad;
};

Texture2D EyeTexture : register(t0);
SamplerState EyeSampler : register(s0);

float3 srgb_to_linear(float3 c)
{
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

float4 main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    float4 colour = EyeTexture.Sample(EyeSampler, uv);
    if (g_linearize != 0)
    {
        colour.rgb = srgb_to_linear(colour.rgb);
    }
    return colour;
}
