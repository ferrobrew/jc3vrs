// A thin C shim over the FSR2 DX11 backend, exposing just what the JC3VRS payload needs as a flat C
// API of opaque pointers and PODs -- so the Rust side never has to transcribe FSR's by-value structs
// (FfxResource, the ~30-field dispatch description, wchar_t name arrays). Everything Ffx stays on the
// C++ side of this boundary; Rust binds the handful of functions below.

#pragma once

#include <stdbool.h>
#include <stdint.h>

#if defined(__cplusplus)
extern "C" {
#endif

// Opaque per-eye FSR context: owns one FfxFsr2Context, its scratch buffer, and the abstract device.
typedef struct FsrContext FsrContext;

// Forward-declared D3D11 types so the header is includable without <d3d11.h> (Rust passes the
// `windows` crate's raw pointers through as `void*`).
typedef struct ID3D11Device ID3D11Device;
typedef struct ID3D11Resource ID3D11Resource;

// Mirrors the subset of FfxFsr2InitializationFlagBits the integration sets. Combined as a bitmask.
typedef enum FsrInitFlags {
    FSR_ENABLE_HIGH_DYNAMIC_RANGE = (1 << 0),
    FSR_ENABLE_DEPTH_INVERTED = (1 << 3),
    FSR_ENABLE_DEPTH_INFINITE = (1 << 4),
    FSR_ENABLE_AUTO_EXPOSURE = (1 << 5),
} FsrInitFlags;

// Create a context for one eye. `flags` is a mask of FsrInitFlags. `maxRenderWidth/Height` bound the
// render-resolution inputs; `displayWidth/Height` is the output (== render size for native AA).
// Returns NULL on failure.
FsrContext* fsr_context_create(
    ID3D11Device* device,
    uint32_t flags,
    uint32_t maxRenderWidth,
    uint32_t maxRenderHeight,
    uint32_t displayWidth,
    uint32_t displayHeight);

// The inputs and per-frame camera parameters for one dispatch. All textures are raw ID3D11Resource*
// (the engine's MainColor/MainDepth/Velocity and our output RT); `exposure`/`reactive` may be NULL.
typedef struct FsrDispatchParams {
    ID3D11Resource* color;        // render-res scene color
    ID3D11Resource* depth;        // render-res depth (reverse-Z if FSR_ENABLE_DEPTH_INVERTED)
    ID3D11Resource* motionVectors; // render-res screen-space velocity
    ID3D11Resource* exposure;     // optional 1x1 exposure, or NULL
    ID3D11Resource* output;       // display-res output color (our RT)

    uint32_t renderWidth;         // resolution the inputs were rendered at
    uint32_t renderHeight;

    float jitterX;                // subpixel jitter applied to the camera this frame
    float jitterY;
    float motionVectorScaleX;     // scale mapping the velocity buffer into FSR's convention
    float motionVectorScaleY;

    bool enableSharpening;
    float sharpness;              // 0..1
    float frameTimeDeltaMs;       // ms since last frame
    float preExposure;            // > 0
    bool reset;                   // true on a camera cut / discontinuity

    float cameraNear;
    float cameraFar;
    float cameraFovAngleVertical; // radians
} FsrDispatchParams;

// Record one FSR2 dispatch onto the device's immediate context. Returns true on success.
bool fsr_context_dispatch(FsrContext* ctx, const FsrDispatchParams* params);

// Destroy a context (the GPU should be idle / done with its resources first).
void fsr_context_destroy(FsrContext* ctx);

// Jitter helpers, passed through from the FSR2 API so the caller can drive the camera with FSR's own
// Halton sequence. `fsr_jitter_phase_count` returns the sequence length for the given resolutions;
// `fsr_jitter_offset` writes the (x, y) offset for `index` within `phaseCount`.
int32_t fsr_jitter_phase_count(int32_t renderWidth, int32_t displayWidth);
void fsr_jitter_offset(float* outX, float* outY, int32_t index, int32_t phaseCount);

#if defined(__cplusplus)
}
#endif
