// HUD quad vertex shader: emits a textured quad from four precomputed clip-space corners.
//
// The corners are computed CPU-side per eye (NDC position + UV), so the shader is a pass-through
// indexed by SV_VertexID over a four-vertex triangle strip. No vertex buffer or input layout.

cbuffer Quad : register(b0)
{
    float4 Corners[4]; // .xy = NDC position, .zw = UV
};

struct VSOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut main(uint vertex_id : SV_VertexID)
{
    VSOut output;
    // Depth is unused -- the quad is an overlay drawn with the depth test disabled.
    output.position = float4(Corners[vertex_id].xy, 0.0, 1.0);
    output.uv = Corners[vertex_id].zw;
    return output;
}
