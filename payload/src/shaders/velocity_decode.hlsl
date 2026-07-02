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
//
// In stereo the engine's velocity is also mis-anchored: the current clip position uses the per-eye
// view-projection, but the previous-frame reprojection uses the single sim-side *center*
// view-projection, so every static pixel gains a spurious parallax vector (depth-dependent, opposite
// sign per eye) and FSR mis-reprojects each eye's temporal history. When CorrectionEnabled is set,
// the pixel is reprojected into the previous frame with both matrices and the difference re-anchors
// the vector at this eye's own previous pose:
//   corrected = decoded + (prevUV_center - prevUV_eye)
// Dynamic objects keep their object motion -- the correction only swaps the camera term.

Texture2D<float4> Velocity : register(t0);
Texture2D<float> Depth : register(t1);
RWTexture2D<float2> Output : register(u0);

cbuffer Params : register(b0)
{
    uint2 Size;             // render resolution (pixels)
    float2 SignScale;       // applied to the decoded UV motion (sign/axis convention; e.g. (1,-1))
    float4x4 ReprojCenter;  // current clip -> previous-frame clip, center camera (what the engine encoded)
    float4x4 ReprojEye;     // current clip -> previous-frame clip, this eye's camera (what FSR wants)
    uint CorrectionEnabled; // 0 = pass the decode through (no stereo disparity, or no history yet)
    // The constant UV offset the camera jitter contributed to every stored vector (FSR wants
    // jitter-free motion; the engine measures curUV under the jittered projection). Subtracted from
    // the decoded motion; zero when jitter is off or the cancellation is disabled.
    float2 JitterUv;
    uint Padding;
};

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Size.x || id.y >= Size.y)
    {
        return;
    }
    float2 stored = Velocity.Load(int3(id.xy, 0)).xy;
    float2 motion_uv = (stored - 0.5) * 0.25 - JitterUv;
    if (CorrectionEnabled != 0)
    {
        // Reconstruct the pixel's clip position from the raster depth (the matrices match the raster
        // view-projection exactly, so the reverse-Z convention passes straight through), reproject it
        // into the previous frame with both cameras, and add the UV-space difference. At the infinite
        // far plane the parallax vanishes and the correction naturally tends to zero.
        float depth = Depth.Load(int3(id.xy, 0)).x;
        float2 uv = (float2(id.xy) + 0.5) / float2(Size);
        float4 clip = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
        float4 prev_center = mul(ReprojCenter, clip);
        float4 prev_eye = mul(ReprojEye, clip);
        float2 delta_ndc = prev_center.xy / prev_center.w - prev_eye.xy / prev_eye.w;
        motion_uv += delta_ndc * float2(0.5, -0.5);
    }
    Output[id.xy] = motion_uv * SignScale;
}
