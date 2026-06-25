// Capture composite vertex shader: emits a single fullscreen triangle from SV_VertexID.
//
// The pixel shader samples one eye's back-buffer capture. The viewport is set per draw to the left
// or right half of the capture swapchain's back buffer, so the same triangle covers whichever half
// is active. No vertex buffer or input layout is needed -- everything is generated from the vertex
// id.

struct VSOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut main(uint vertex_id : SV_VertexID)
{
    VSOut output;
    // id 0 -> (-1, -1), id 1 -> (3, -1), id 2 -> (-1, 3): one triangle covering the whole screen.
    output.position = float4(
        (vertex_id == 1) ? 3.0 : -1.0,
        (vertex_id == 2) ? 3.0 : -1.0,
        0.0,
        1.0);
    // UVs: (0,1), (2,1), (0,-1). D3D render-target textures are top-down (V=0 = top row), but clip
    // space Y=+1 is the top of the screen, so we map the bottom of the screen (clip Y=-1) to V=1
    // (bottom of the texture) and the top to V=0. The (0,-1) corner sits off-screen above the top and
    // clamps to V=0 via CLAMP addressing, so it never samples past the texture's top edge.
    output.uv = float2((vertex_id == 1) ? 2.0 : 0.0, (vertex_id == 2) ? -1.0 : 1.0);
    return output;
}
