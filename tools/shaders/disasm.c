// Minimal DXBC disassembler: reads a .dxbc blob and prints its SM5 assembly to stdout.
//
// Loads d3dcompiler_47.dll at runtime (LoadLibrary/GetProcAddress) so it links against nothing but
// kernel32 -- no d3dcompiler import library needed. Build it for Windows and run it under wine; see
// disasm.sh, which builds this with clang + the repo's xwin sysroot and runs it against the
// d3dcompiler_47.dll that `cargo run -p shadergen` provisions.

#include <windows.h>
#include <d3dcommon.h>
#include <stdio.h>
#include <stdlib.h>

typedef HRESULT(WINAPI *PFN_D3DDisassemble)(LPCVOID, SIZE_T, UINT, LPCSTR, ID3DBlob **);

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: disasm <file.dxbc>\n");
        return 2;
    }
    FILE *f = fopen(argv[1], "rb");
    if (!f) {
        fprintf(stderr, "disasm: cannot open %s\n", argv[1]);
        return 2;
    }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    void *buf = malloc(n);
    if (fread(buf, 1, n, f) != (size_t)n) {
        fprintf(stderr, "disasm: short read\n");
        return 2;
    }
    fclose(f);

    HMODULE h = LoadLibraryA("d3dcompiler_47.dll");
    if (!h) {
        fprintf(stderr, "disasm: d3dcompiler_47.dll not found (must sit next to the exe)\n");
        return 3;
    }
    PFN_D3DDisassemble dis = (PFN_D3DDisassemble)GetProcAddress(h, "D3DDisassemble");
    if (!dis) {
        fprintf(stderr, "disasm: no D3DDisassemble export\n");
        return 3;
    }

    ID3DBlob *out = NULL;
    HRESULT hr = dis(buf, n, 0, NULL, &out);
    if (hr != 0 || !out) {
        // hr 0x80004005 here usually means the container hash no longer matches the body, e.g. after a
        // raw byte-patch; D3DDisassemble validates it even though the runtime does not.
        fprintf(stderr, "disasm: D3DDisassemble failed (hr=0x%lx)\n", (unsigned long)hr);
        return 4;
    }
    fwrite(out->lpVtbl->GetBufferPointer(out), 1, out->lpVtbl->GetBufferSize(out), stdout);
    return 0;
}
