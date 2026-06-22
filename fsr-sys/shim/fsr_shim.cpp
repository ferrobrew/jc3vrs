// Implementation of the FSR2 DX11 C shim (see fsr_shim.h). Confines all the Ffx* struct handling to
// C++; translates the flat C API into FSR2 backend calls.

#include "fsr_shim.h"

#include <d3d11.h>
#include <new>

#include "ffx_fsr2.h"
#include "dx11/ffx_fsr2_dx11.h"

struct FsrContext {
    FfxFsr2Context fsr;
    void* scratch;
    FfxDevice device;
};

FsrContext* fsr_context_create(
    ID3D11Device* device,
    uint32_t flags,
    uint32_t maxRenderWidth,
    uint32_t maxRenderHeight,
    uint32_t displayWidth,
    uint32_t displayHeight)
{
    if (device == nullptr) {
        return nullptr;
    }

    FsrContext* ctx = new (std::nothrow) FsrContext{};
    if (ctx == nullptr) {
        return nullptr;
    }

    const size_t scratchSize = ffxFsr2GetScratchMemorySizeDX11();
    ctx->scratch = malloc(scratchSize);
    if (ctx->scratch == nullptr) {
        delete ctx;
        return nullptr;
    }

    FfxFsr2ContextDescription desc = {};
    desc.flags = flags;
    desc.maxRenderSize.width = maxRenderWidth;
    desc.maxRenderSize.height = maxRenderHeight;
    desc.displaySize.width = displayWidth;
    desc.displaySize.height = displayHeight;
    desc.device = ffxGetDeviceDX11(device);

    FfxErrorCode err = ffxFsr2GetInterfaceDX11(&desc.callbacks, device, ctx->scratch, scratchSize);
    if (err != FFX_OK) {
        free(ctx->scratch);
        delete ctx;
        return nullptr;
    }
    ctx->device = desc.device;

    err = ffxFsr2ContextCreate(&ctx->fsr, &desc);
    if (err != FFX_OK) {
        free(ctx->scratch);
        delete ctx;
        return nullptr;
    }
    return ctx;
}

bool fsr_context_dispatch(FsrContext* ctx, const FsrDispatchParams* p)
{
    if (ctx == nullptr || p == nullptr) {
        return false;
    }

    FfxFsr2DispatchDescription desc = {};
    // DX11 records onto the device's immediate context; the backend resolves it from the device, so
    // the command list is unused (pass null).
    desc.commandList = nullptr;

    desc.color = ffxGetResourceDX11(&ctx->fsr, p->color, nullptr, FFX_RESOURCE_STATE_COMPUTE_READ);
    desc.depth = ffxGetResourceDX11(&ctx->fsr, p->depth, nullptr, FFX_RESOURCE_STATE_COMPUTE_READ);
    desc.motionVectors =
        ffxGetResourceDX11(&ctx->fsr, p->motionVectors, nullptr, FFX_RESOURCE_STATE_COMPUTE_READ);
    desc.exposure = p->exposure != nullptr
        ? ffxGetResourceDX11(&ctx->fsr, p->exposure, nullptr, FFX_RESOURCE_STATE_COMPUTE_READ)
        : FfxResource{};
    desc.reactive = FfxResource{};
    desc.transparencyAndComposition = FfxResource{};
    desc.output =
        ffxGetResourceDX11(&ctx->fsr, p->output, nullptr, FFX_RESOURCE_STATE_UNORDERED_ACCESS);

    desc.jitterOffset.x = p->jitterX;
    desc.jitterOffset.y = p->jitterY;
    desc.motionVectorScale.x = p->motionVectorScaleX;
    desc.motionVectorScale.y = p->motionVectorScaleY;
    desc.renderSize.width = p->renderWidth;
    desc.renderSize.height = p->renderHeight;
    desc.enableSharpening = p->enableSharpening;
    desc.sharpness = p->sharpness;
    desc.frameTimeDelta = p->frameTimeDeltaMs;
    desc.preExposure = p->preExposure;
    desc.reset = p->reset;
    desc.cameraNear = p->cameraNear;
    desc.cameraFar = p->cameraFar;
    desc.cameraFovAngleVertical = p->cameraFovAngleVertical;
    desc.viewSpaceToMetersFactor = 1.0f;

    return ffxFsr2ContextDispatch(&ctx->fsr, &desc) == FFX_OK;
}

void fsr_context_destroy(FsrContext* ctx)
{
    if (ctx == nullptr) {
        return;
    }
    ffxFsr2ContextDestroy(&ctx->fsr);
    free(ctx->scratch);
    delete ctx;
}

int32_t fsr_jitter_phase_count(int32_t renderWidth, int32_t displayWidth)
{
    return ffxFsr2GetJitterPhaseCount(renderWidth, displayWidth);
}

void fsr_jitter_offset(float* outX, float* outY, int32_t index, int32_t phaseCount)
{
    ffxFsr2GetJitterOffset(outX, outY, index, phaseCount);
}
