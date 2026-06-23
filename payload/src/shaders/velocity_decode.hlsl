// Decode JC3's bias-encoded velocity buffer into an unbiased R16G16F motion-vector buffer for FSR.
//
// JC3's velocity buffer (m_VelocityBufferTexture, ABGR8) stores, per the RE'd velocity-write shader:
//   stored.xy = clamp((curUV - prevUV) * 8, -1, 1) * 0.5 + 0.5
// i.e. screen-space motion in UV units (Y-down), scaled x8, clamped to +/-1, packed into [0,1] with
// 0.5 == zero motion. FSR's motionVectorScale is a pure multiply and cannot subtract the 0.5 bias, so
// we decode here:
//   motion_uv = (stored.xy - 0.5) * 0.25     // = curUV - prevUV, UV space, Y-down
// then apply FSR's sign/axis convention via the SignScale constant (runtime-tunable so the sign/Y can
// be settled without recompiling), and write UV-space motion. The dispatch's motionVectorScale then
// maps UV -> pixels: (renderWidth, renderHeight) * SignScale handled CPU-side.

Texture2D<float4> Velocity : register(t0);
RWTexture2D<float2> Output : register(u0);

cbuffer Params : register(b0)
{
    uint2 Size;       // render resolution (pixels)
    float2 SignScale; // applied to the decoded UV motion (sign/axis convention; e.g. (1,-1))
};

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Size.x || id.y >= Size.y)
    {
        return;
    }
    float2 stored = Velocity.Load(int3(id.xy, 0)).xy;
    float2 motion_uv = (stored - 0.5) * 0.25;
    Output[id.xy] = motion_uv * SignScale;
}
