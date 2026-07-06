// Virtual mouse cursor pixel shader: an analytic circle dot with a stroke, drawn on a small quad
// floating just off the HUD panel (shares hud_quad_vs). The dot is a light fill ringed by a dark
// stroke so it reads against both bright and dark UI, antialiased against the quad's UV-space
// pixel footprint.

static const float OuterRadius = 0.42;   // stroke's outer edge, in UV units from the center
static const float StrokeWidth = 0.10;   // stroke thickness, in UV units
static const float3 FillColor = float3(1.0, 1.0, 1.0);
static const float3 StrokeColor = float3(0.08, 0.08, 0.08);
static const float Opacity = 0.95;

float4 main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    float r = length(uv - 0.5);
    float aa = max(fwidth(r), 1e-5);

    float inner_radius = OuterRadius - StrokeWidth;
    float coverage = 1.0 - smoothstep(OuterRadius - aa, OuterRadius + aa, r);
    float fill_mask = 1.0 - smoothstep(inner_radius - aa, inner_radius + aa, r);

    float3 color = lerp(StrokeColor, FillColor, fill_mask);
    return float4(color, coverage * Opacity);
}
