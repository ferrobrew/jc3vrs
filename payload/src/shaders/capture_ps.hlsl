// Capture composite pixel shader: samples one eye's back-buffer capture into its half of the
// side-by-side presentation surface.

Texture2D EyeTexture : register(t0);
SamplerState EyeSampler : register(s0);

float4 main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    return EyeTexture.Sample(EyeSampler, uv);
}
