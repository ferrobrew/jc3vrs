// HUD quad pixel shader: samples the redirected HUD texture for the floating panel.

Texture2D HudTexture : register(t0);
SamplerState HudSampler : register(s0);

float4 main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    return HudTexture.Sample(HudSampler, uv);
}
