// Depth-distribution histogram for the dynamic HUD distance (issue #14).
//
// Samples the main scene depth buffer on a stride grid, linearizes each sample through the
// projection's z-row (works for standard and reversed-Z alike), and accumulates a log-spaced
// histogram over [BIN_MIN_METERS, BIN_MAX_METERS]. Slot BIN_COUNT holds the total sample count.
// The consumer clears the histogram buffer before dispatch and reads it back asynchronously.

#define BIN_COUNT 32
#define BIN_MIN_METERS 0.25
#define BIN_MAX_METERS 256.0

cbuffer Params : register(b0)
{
    // Depth texture dimensions in pixels.
    float2 DepthDims;
    // Projection z-row terms: device depth d = ProjA + ProjB / view_z, so
    // view_z = ProjB / (d - ProjA).
    float ProjA;
    float ProjB;
    // Sample every Nth pixel on both axes.
    uint Stride;
    uint3 _pad;
}

Texture2D<float> Depth : register(t0);
RWBuffer<uint> Histogram : register(u0);

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    const uint2 pixel = id.xy * Stride;
    if (pixel.x >= (uint) DepthDims.x || pixel.y >= (uint) DepthDims.y)
    {
        return;
    }

    const float device_depth = Depth[pixel];
    float view_z = ProjB / (device_depth - ProjA);
    // Guard degenerate projections and the clear value (view_z can come out negative,
    // infinite, or NaN there); such samples count toward the total as far-field.
    if (!(view_z > 0.0))
    {
        view_z = BIN_MAX_METERS;
    }

    const float t = log2(max(view_z, BIN_MIN_METERS) / BIN_MIN_METERS)
        / log2(BIN_MAX_METERS / BIN_MIN_METERS);
    const uint bin = min((uint) (saturate(t) * BIN_COUNT), BIN_COUNT - 1);

    uint ignored;
    InterlockedAdd(Histogram[bin], 1, ignored);
    InterlockedAdd(Histogram[BIN_COUNT], 1, ignored);
}
