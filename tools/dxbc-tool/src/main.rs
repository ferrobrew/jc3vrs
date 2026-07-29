//! DXBC compile/disassemble harness: thin wrappers over `d3dcompiler_47`'s `D3DCompile` and
//! `D3DDisassemble`, used when designing bytecode transforms (for example, to learn the exact chunks
//! `fxc` emits for a vertex shader that writes `SV_ViewportArrayIndex`).
//!
//! This is a Windows binary (`x86_64-pc-windows-msvc`, the workspace default target) run under Wine
//! against the native `d3dcompiler_47.dll` that `cargo run -p shadergen` provisions; see
//! `scripts/dxbc.sh`. It replaces the earlier hand-written C harnesses (`compile.c` /
//! `disasm.c`): the `windows` crate supplies the `D3DCompile` / `D3DDisassemble` bindings, so there
//! is no manual `LoadLibrary` / `GetProcAddress`, and Rust's stdout writes bytes verbatim -- so the
//! DXBC blob reaches a redirect intact, without the text-mode CR-LF corruption the C version had to
//! guard against.
//!
//! Usage:
//!   dxbc-tool compile <file.hlsl> <entry> <target>   # writes the DXBC blob to stdout
//!   dxbc-tool disasm  <file.dxbc>                     # writes SM5 assembly to stdout

use std::{ffi::CString, fmt, io::Write, process::ExitCode};

use windows::{
    Win32::Graphics::Direct3D::{
        Fxc::{D3DCompile, D3DDisassemble},
        ID3DBlob,
    },
    core::PCSTR,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code())
        }
    }
}

/// Dispatches the subcommand and streams the resulting blob to stdout.
fn run(args: &[String]) -> Result<(), ToolError> {
    match args.first().map(String::as_str) {
        Some("compile") => match args {
            [_, file, entry, target] => {
                let blob = compile(file, entry, target)?;
                write_stdout(blob_bytes(&blob))
            }
            _ => Err(ToolError::Usage),
        },
        Some("disasm") => match args {
            [_, file] => {
                let blob = disassemble(file)?;
                write_stdout(blob_bytes(&blob))
            }
            _ => Err(ToolError::Usage),
        },
        _ => Err(ToolError::Usage),
    }
}

/// Compiles an HLSL source file to a DXBC blob. On a compile error the diagnostics blob is surfaced
/// in the returned error, mirroring what `fxc` prints.
fn compile(file: &str, entry: &str, target: &str) -> Result<ID3DBlob, ToolError> {
    let src = read_file(file)?;
    let name = cstring(file, file)?;
    let entry = cstring(entry, file)?;
    let target = cstring(target, file)?;

    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // Flags 0/0 = default optimizations, to stay close to how the game's shaders were built.
    let result = unsafe {
        D3DCompile(
            src.as_ptr().cast(),
            src.len(),
            PCSTR(name.as_ptr().cast()),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };

    match result {
        Ok(()) => code.ok_or(ToolError::EmptyOutput { op: "D3DCompile" }),
        Err(e) => Err(ToolError::Compile {
            hr: e.code().0,
            diagnostics: errors.as_ref().map(blob_string).unwrap_or_default(),
        }),
    }
}

/// Disassembles a DXBC blob to SM5 assembly text.
fn disassemble(file: &str) -> Result<ID3DBlob, ToolError> {
    let data = read_file(file)?;
    // `D3DDisassemble` validates the container hash even though the runtime does not, so a raw
    // byte-patched blob fails here with `0x80004005` until its checksum is refreshed.
    unsafe { D3DDisassemble(data.as_ptr().cast(), data.len(), 0, PCSTR::null()) }
        .map_err(|e| ToolError::Disassemble { hr: e.code().0 })
}

/// The blob's bytes as a slice borrowed from the blob.
fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        let ptr = blob.GetBufferPointer().cast::<u8>();
        std::slice::from_raw_parts(ptr, blob.GetBufferSize())
    }
}

/// The blob's bytes as a string, trimming the trailing NUL of a C-string diagnostics blob.
fn blob_string(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob))
        .trim_end_matches(['\0', '\n', '\r'])
        .to_string()
}

fn write_stdout(bytes: &[u8]) -> Result<(), ToolError> {
    std::io::stdout()
        .write_all(bytes)
        .map_err(|e| ToolError::Stdout(e.to_string()))
}

fn read_file(path: &str) -> Result<Vec<u8>, ToolError> {
    std::fs::read(path).map_err(|e| ToolError::Read {
        path: path.to_string(),
        source: e.to_string(),
    })
}

/// Builds a NUL-terminated C string, erroring on any interior NUL byte rather than silently stripping
/// it, since a NUL in a shader source or entry point is almost certainly a mistake.
fn cstring(s: &str, path: &str) -> Result<CString, ToolError> {
    CString::new(s).map_err(|_| ToolError::InteriorNul {
        path: path.to_string(),
    })
}

/// A tool error, carrying the exit code its category maps to.
enum ToolError {
    Usage,
    Read { path: String, source: String },
    Stdout(String),
    Compile { hr: i32, diagnostics: String },
    Disassemble { hr: i32 },
    EmptyOutput { op: &'static str },
    InteriorNul { path: String },
}

impl ToolError {
    fn exit_code(&self) -> u8 {
        match self {
            ToolError::Usage | ToolError::Read { .. } | ToolError::Stdout(_) => 2,
            ToolError::Compile { .. }
            | ToolError::Disassemble { .. }
            | ToolError::EmptyOutput { .. }
            | ToolError::InteriorNul { .. } => 4,
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::Usage => f.write_str(
                "dxbc-tool: usage: compile <file.hlsl> <entry> <target> | disasm <file.dxbc>",
            ),
            ToolError::Read { path, source } => {
                write!(f, "dxbc-tool: cannot read {path}: {source}")
            }
            ToolError::Stdout(source) => {
                write!(f, "dxbc-tool: writing to stdout failed: {source}")
            }
            ToolError::Compile { hr, diagnostics } if diagnostics.is_empty() => {
                write!(f, "dxbc-tool: compile: D3DCompile failed (hr=0x{hr:08x})")
            }
            ToolError::Compile { hr, diagnostics } => {
                write!(
                    f,
                    "dxbc-tool: compile: D3DCompile failed (hr=0x{hr:08x}):\n{diagnostics}"
                )
            }
            ToolError::Disassemble { hr } => {
                write!(
                    f,
                    "dxbc-tool: disasm: D3DDisassemble failed (hr=0x{hr:08x})"
                )
            }
            ToolError::EmptyOutput { op } => {
                write!(f, "dxbc-tool: {op} succeeded but produced no output blob")
            }
            ToolError::InteriorNul { path } => {
                write!(
                    f,
                    "dxbc-tool: the shader source at {path} contains a NUL byte"
                )
            }
        }
    }
}
