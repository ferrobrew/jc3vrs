// HUD marker-layer vertex shader: a grid mesh over the panel, depth-warped per marker.
//
// The marker layer's texture holds world-anchored markers whose true depths the CPU recorded from
// the game's own world-to-screen calls. Every grid vertex is displaced along the ray from the
// panel anchor (the head position the corners were built around) through its flat position: since
// each point stays on its own anchor ray, the cyclopean image is identical no matter what the
// depth field does -- only the stereo disparity varies across the panel. Depths are blended in
// disparity (1/d) space, the quantity disparity is actually linear in, with the flat panel
// distance as the base and a smooth radial falloff around each marker.
//
// Grid: GridSize.x * GridSize.y cells, six vertices per cell (two triangles), generated from
// SV_VertexID with no vertex buffer. `row_major` matches the engine's row-major layout.

cbuffer Layer : register(b0)
{
    row_major float4x4 ViewProjection;
    float4 Corners[4];   // .xyz = world-space corner (TL, TR, BL, BR), .w = unused
    float4 AnchorBase;   // .xyz = panel anchor (head position), .w = base (flat) distance
    uint4 GridSize;      // .x = columns, .y = rows, .z = marker count, .w = unused
    float4 Markers[32];  // .x = u, .y = v, .z = world depth (m), .w = falloff radius (uv units)
};

struct VSOut
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

// The uv of a grid vertex: cell + within-cell corner from the vertex id, as two triangles
// (0,1,2 / 2,1,3 in row-major corner order 0=TL 1=TR 2=BL 3=BR).
float2 grid_uv(uint vertex_id)
{
    uint cell = vertex_id / 6;
    uint corner_index = vertex_id % 6;
    uint2 corner_offsets[6] =
    {
        uint2(0, 0), uint2(1, 0), uint2(0, 1),
        uint2(0, 1), uint2(1, 0), uint2(1, 1),
    };
    uint2 cell_pos = uint2(cell % GridSize.x, cell / GridSize.x);
    uint2 vertex_pos = cell_pos + corner_offsets[corner_index];
    return float2(vertex_pos) / float2(GridSize.xy);
}

VSOut main(uint vertex_id : SV_VertexID)
{
    VSOut output;
    float2 uv = grid_uv(vertex_id);

    // The flat panel position for this uv (bilinear over the world-space corners).
    float3 top = lerp(Corners[0].xyz, Corners[1].xyz, uv.x);
    float3 bottom = lerp(Corners[2].xyz, Corners[3].xyz, uv.x);
    float3 flat_pos = lerp(top, bottom, uv.y);

    // Blend the depth field in disparity space: base disparity at full weight remainder, each
    // marker contributing its own disparity under a radial falloff. An edge fade keeps the panel
    // borders (and anything drawn there) at the flat depth.
    float base_distance = max(AnchorBase.w, 0.01);
    float edge_fade =
        smoothstep(0.0, 0.08, uv.x) * smoothstep(1.0, 0.92, uv.x) *
        smoothstep(0.0, 0.08, uv.y) * smoothstep(1.0, 0.92, uv.y);
    float marker_weight_total = 0.0;
    float disparity_acc = 0.0;
    uint count = min(GridSize.z, 32u);
    for (uint i = 0; i < count; ++i)
    {
        float2 marker_uv = Markers[i].xy;
        float depth = max(Markers[i].z, 0.01);
        float radius = max(Markers[i].w, 0.001);
        float w = (1.0 - smoothstep(0.0, radius, length(uv - marker_uv))) * edge_fade;
        disparity_acc += w / depth;
        marker_weight_total += w;
    }
    float base_weight = max(1.0 - marker_weight_total, 0.0);
    float disparity = (base_weight / base_distance + disparity_acc)
        / max(base_weight + marker_weight_total, 1e-4);
    float depth_here = 1.0 / max(disparity, 1e-4);

    // Displace along the anchor ray: direction is preserved exactly, only disparity changes.
    float3 world_pos = AnchorBase.xyz + (flat_pos - AnchorBase.xyz) * (depth_here / base_distance);

    output.position = mul(float4(world_pos, 1.0), ViewProjection);
    output.uv = uv;
    return output;
}
