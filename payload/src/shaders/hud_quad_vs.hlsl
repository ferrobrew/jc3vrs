// HUD quad vertex shader: projects four world-space corners through the camera's view-projection.
//
// Corners are computed CPU-side in view space, transformed to world space, then uploaded. The
// view-projection matrix (with reverse-Z applied) transforms them to clip space. UVs are hardcoded
// per vertex ID. `row_major` matches the engine's row-major data layout so `mul(v, M)` = `v * M`.

cbuffer Quad : register(b0)
{
    row_major float4x4 ViewProjection;
    float4 Corners[4]; // .xyz = world-space position, .w = unused
};

static const float2 Uvs[4] =
{
    float2(0.0, 0.0),
    float2(1.0, 0.0),
    float2(0.0, 1.0),
    float2(1.0, 1.0),
};

struct VSOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut main(uint vertex_id : SV_VertexID)
{
    VSOut output;
    float3 world_pos = Corners[vertex_id].xyz;
    output.position = mul(float4(world_pos, 1.0), ViewProjection);
    output.uv = Uvs[vertex_id];
    return output;
}
