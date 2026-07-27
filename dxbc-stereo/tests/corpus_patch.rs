//! Validates the single-pass vertex-shader rewrite against the game's *entire* real shader set.
//!
//! Where `patch.rs` checks the transform on one canonical shader and `census.rs` checks the operand
//! analysis over the corpus, this runs the full [`patch_vertex_shader`] over every shader in the
//! extracted bundle and structurally validates each success: the output re-parses, carries the
//! `SFI0` viewport-routing feature bit, has **no** residual per-eye `cb0` operand (every one was
//! remapped to `cb13`), and its checksum is self-consistent. It is the offline proof that the
//! rewriter survives contact with the real shader set before any of it reaches the game.
//!
//! The bundle is game-derived and git-ignored, so the tests read the local extract
//! (`tools/shaders/Shaders_F.shaders/`). A missing extract is a **failure**, not a skip -- see
//! [`common::ALLOW_MISSING`].

use std::collections::BTreeMap;

use dxbc_stereo::{
    Dxbc, DxbcError, OperandKind, SSDECAL_EYE_BIAS_REGISTER, ShaderStage, TokenStream,
    bias_ssdecal_depth_uv, patch_vertex_shader, per_eye_refs, reads_global_view_projection,
};

mod common;
use common::shader_dir;

/// `SFI0` bit 13 -- `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer`, which every patched shader
/// must declare so its `SV_ViewportArrayIndex` output is valid.
const SFI0_VIEWPORT_BIT: u64 = 1 << 13;

/// Whether a blob is a vertex shader (an `SHEX`/`SHDR` chunk whose program type is vertex).
fn is_vertex_shader(blob: &[u8]) -> bool {
    Dxbc::parse(blob)
        .ok()
        .and_then(|dxbc| dxbc.shader_chunk())
        .and_then(|shex| TokenStream::new(shex.body(blob)).ok())
        .is_some_and(|stream| stream.stage() == ShaderStage::Vertex)
}

/// Structurally validate one patched blob. Returns `Err(reason)` on the first invariant that fails.
fn validate_patch(patched: &[u8]) -> Result<(), String> {
    let dxbc = Dxbc::parse(patched).map_err(|e| format!("output does not re-parse: {e}"))?;

    let sfi0 = dxbc
        .chunk(b"SFI0")
        .ok_or_else(|| "output has no SFI0 chunk".to_string())?;
    let body = sfi0.body(patched);
    if body.len() < 8 {
        return Err(format!("SFI0 body is {} bytes, expected >= 8", body.len()));
    }
    let flags = u64::from_le_bytes(body[..8].try_into().expect("8 bytes"));
    if flags & SFI0_VIEWPORT_BIT == 0 {
        return Err(format!("SFI0 viewport bit not set (flags {flags:#x})"));
    }

    // The whole point of the rewrite: no per-eye cb0 operand may survive -- each must have become a
    // cb13 reference. A non-empty result means the transform missed an operand.
    let residual =
        per_eye_refs(patched).map_err(|e| format!("per_eye_refs on the output failed: {e}"))?;
    if !residual.is_empty() {
        let rows: Vec<u32> = residual.iter().map(|r| r.row).collect();
        return Err(format!(
            "{} residual per-eye cb0 operands at rows {rows:?}",
            residual.len()
        ));
    }

    // The token stream must still walk cleanly end to end.
    let shex = dxbc
        .shader_chunk()
        .ok_or_else(|| "output has no shader chunk".to_string())?;
    let stream = TokenStream::new(shex.body(patched))
        .map_err(|e| format!("output SHEX does not parse: {e}"))?;
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("output instruction walk failed: {e}"))?;
        for operand in insn.operands() {
            operand.map_err(|e| format!("output operand walk failed: {e}"))?;
        }
    }

    Ok(())
}

/// Run the full rewrite over every vertex shader in the bundle: every success must validate, and the
/// only tolerated failures are the two documented deferrals -- `NoPerEyeReferences` (the baked-WVP /
/// no-position families) and `InstanceIdAlreadyDeclared` (shaders that already consume
/// `SV_InstanceID` for their own instancing, whose `>> 1` consumer rewrite is a later phase). Both
/// are left double-drawn. Any *other* error, or any structurally-invalid output, fails the test with
/// the offending shader named.
#[test]
fn corpus_patch_is_sound() {
    let Some(dir) = shader_dir() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };

    let mut vs_total = 0usize;
    let mut patched = 0usize;
    let mut no_refs = 0usize;
    let mut instance_id = 0usize;
    let mut other_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut invalid: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        if !is_vertex_shader(&blob) {
            continue;
        }
        vs_total += 1;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        match patch_vertex_shader(&blob) {
            Ok(out) => {
                patched += 1;
                if let Err(reason) = validate_patch(&out) {
                    invalid.push(format!("{name}: {reason}"));
                }
            }
            Err(DxbcError::NoPerEyeReferences) => no_refs += 1,
            Err(DxbcError::InstanceIdAlreadyDeclared) => instance_id += 1,
            Err(e) => other_errors.entry(e.to_string()).or_default().push(name),
        }
    }

    eprintln!(
        "corpus patch: {vs_total} VS -> {patched} patched, {no_refs} no-per-eye-refs, \
         {instance_id} instance-id-deferred, {} other-errored, {} structurally-invalid",
        other_errors.values().map(Vec::len).sum::<usize>(),
        invalid.len(),
    );
    for (err, shaders) in &other_errors {
        eprintln!(
            "  error kind '{err}': {} shaders, e.g. {:?}",
            shaders.len(),
            &shaders[..shaders.len().min(3)]
        );
    }
    for line in &invalid {
        eprintln!("  invalid {line}");
    }

    assert_eq!(vs_total, 455, "total vertex shaders in the bundle");
    assert!(
        invalid.is_empty(),
        "{} patched shaders are structurally invalid",
        invalid.len()
    );
    assert!(
        other_errors.is_empty(),
        "{} shaders errored outside the two documented deferrals",
        other_errors.values().map(Vec::len).sum::<usize>(),
    );
    assert_eq!(
        patched + no_refs + instance_id,
        455,
        "every VS is patched, has no per-eye refs, or is an instance-id deferral"
    );
}

/// The vegetation vertex shaders that read `cb0` read **only** `cb0[4]`, never the view-projection
/// rows -- so the `cb0` remap claims them on the strength of a reference that is not a clip-position
/// input, and remapping it moves a wind-noise origin (foliage) or a camera-relative offset paired with
/// a *baked* view-projection (bark) rather than giving the shader a per-eye clip position.
///
/// This is the fact the payload's baked-cb block intercepts depend on: they own the vegetation blocks'
/// draws and reproject the baked matrix themselves, which only works while the rewriter leaves those
/// shaders alone. If a future bundle gives one of them a real `cb0[29..32]` clip path, the decline is
/// wrong for it and this test says so.
///
/// The blobs are named by their index and offset in the bundle, which are stable for the shipped
/// bundle; `tools/shaders/shader_names.py` maps them to the engine's own names.
#[test]
fn corpus_vegetation_reads_the_camera_position_but_never_the_view_projection() {
    let Some(dir) = shader_dir() else {
        return;
    };

    /// Every `vegetationfoliage*` / `vegetationbark*` vertex shader that references `cb0` at all.
    const VEGETATION: &[&str] = &[
        "sh_0238_00093d90.dxbc", // vegetationbarkhwinstanced
        "sh_0239_000949b0.dxbc", // vegetationbarkinstanced
        "sh_0241_00095e30.dxbc", // vegetationbarkprezhwinstanced
        "sh_0242_00096530.dxbc", // vegetationbarkprezinstanced
        "sh_0244_000973e0.dxbc", // vegetationbarkshadowhwinstanced
        "sh_0245_00097ae0.dxbc", // vegetationbarkshadowinstanced
        "sh_0247_00098ad0.dxbc", // vegetationbarkvelocityhwinstanced
        "sh_0248_00099300.dxbc", // vegetationbarkvelocityinstanced
        "sh_0249_00099e00.dxbc", // vegetationfoliage
        "sh_0251_0009bdb0.dxbc", // vegetationfoliagehwinstanced
        "sh_0252_0009d960.dxbc", // vegetationfoliagehwinstanced_osnormalmap
        "sh_0253_0009f330.dxbc", // vegetationfoliageinstanced
        "sh_0254_000a1270.dxbc", // vegetationfoliageinstanced_osnormalmap
        "sh_0255_000a2f00.dxbc", // vegetationfoliageprez
        "sh_0256_000a3d00.dxbc", // vegetationfoliageprezhwinstanced
        "sh_0257_000a5160.dxbc", // vegetationfoliageprezinstanced
        "sh_0261_000a86c0.dxbc", // vegetationfoliageshadow
        "sh_0262_000a8f60.dxbc", // vegetationfoliageshadowhwinstanced
        "sh_0263_000a9e80.dxbc", // vegetationfoliageshadowinstanced
        "sh_0264_000ab050.dxbc", // vegetationfoliagevelocity
        "sh_0265_000abf90.dxbc", // vegetationfoliagevelocityhwinstanced
        "sh_0266_000ad540.dxbc", // vegetationfoliagevelocityinstanced
        "sh_0267_000aeda0.dxbc", // vegetationfoliage_osnormalmap
    ];

    for name in VEGETATION {
        let blob = std::fs::read(dir.join(name)).expect("read vegetation blob");
        let rows: Vec<u32> = per_eye_refs(&blob)
            .expect("parse vegetation blob")
            .iter()
            .map(|r| r.row)
            .collect();
        assert!(
            !rows.is_empty(),
            "{name} has no per-eye cb0 reference at all, so the rewriter never claimed it"
        );
        assert!(
            rows.iter().all(|&r| r == 4),
            "{name} references cb0 rows {rows:?}, not the camera position alone"
        );
        assert!(
            !reads_global_view_projection(&blob).expect("parse vegetation blob"),
            "{name} reads the global view-projection"
        );
        // A permutation that consumes `SV_InstanceID` for its own instancing is deferred by the
        // rewriter rather than claimed, so the decline is a no-op for it -- but it is still declined,
        // because whether a permutation is claimed is not something the name prefix can tell.
        assert!(
            matches!(
                patch_vertex_shader(&blob),
                Ok(_) | Err(DxbcError::InstanceIdAlreadyDeclared)
            ),
            "{name} is neither claimed nor deferred by the cb0 remap: {:?}",
            patch_vertex_shader(&blob).err(),
        );
    }

    // The corpus-wide population the decline is drawn from: shaders the remap claims purely on a
    // `cb0[4]` reference. The payload declines only the vegetation ones, whose draws a block intercept
    // owns; the rest (water, clouds, particles, and a handful of model families) keep the remap, since
    // nothing else re-issues their draws per eye.
    let mut camera_only = 0usize;
    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        if !is_vertex_shader(&blob) {
            continue;
        }
        if !per_eye_refs(&blob).expect("parse blob").is_empty()
            && !reads_global_view_projection(&blob).expect("parse blob")
        {
            camera_only += 1;
        }
    }
    assert_eq!(
        camera_only, 50,
        "vertex shaders the cb0 remap claims on a camera-position reference alone",
    );
}

/// The decal depth-UV bias is offered every fragment shader the engine creates, so its structural
/// matcher has to be exact: it must transform the twelve `ssdecal*` permutations and decline all 949
/// other pixel shaders. Each transform must also re-parse, keep a walkable token stream, and widen the
/// `cb1` declaration far enough to cover the register the injected `mad` reads.
///
/// The count is the whole point. A matcher that over-captures corrupts unrelated shaders in flight,
/// and one that under-captures leaves the decals reading the wrong half of the depth buffer while
/// reporting success.
#[test]
fn corpus_ssdecal_bias_matches_only_the_decal_permutations() {
    let Some(dir) = shader_dir() else {
        return;
    };

    let mut pixel_shaders = 0usize;
    let mut biased: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    let mut other_errors: BTreeMap<String, usize> = BTreeMap::new();

    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        if stage(&blob) != Some(ShaderStage::Pixel) {
            continue;
        }
        pixel_shaders += 1;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        match bias_ssdecal_depth_uv(&blob) {
            Ok(out) => {
                biased.push(name.clone());
                if let Err(reason) = validate_bias(&out) {
                    invalid.push(format!("{name}: {reason}"));
                }
            }
            Err(DxbcError::NoDepthUvFetch) => {}
            Err(e) => *other_errors.entry(e.to_string()).or_default() += 1,
        }
    }

    biased.sort();
    eprintln!(
        "ssdecal bias: {pixel_shaders} PS -> {} biased, {} structurally invalid",
        biased.len(),
        invalid.len(),
    );
    for line in &invalid {
        eprintln!("  invalid {line}");
    }
    assert!(
        invalid.is_empty(),
        "{} biased shaders are invalid",
        invalid.len()
    );
    assert!(
        other_errors.is_empty(),
        "the bias errored outside the documented decline: {other_errors:?}",
    );
    assert_eq!(pixel_shaders, 961, "total pixel shaders in the bundle");
    assert_eq!(
        biased.len(),
        12,
        "the twelve ssdecal* permutations, and nothing else: {biased:?}",
    );
}

/// Structurally validate one biased blob: it re-parses, its token stream walks end to end, and its
/// `cb1` declaration covers the register the injected instruction reads.
fn validate_bias(blob: &[u8]) -> Result<(), String> {
    let dxbc = Dxbc::parse(blob).map_err(|e| format!("output does not re-parse: {e}"))?;
    let shex = dxbc
        .shader_chunk()
        .ok_or_else(|| "output has no shader chunk".to_string())?;
    let stream = TokenStream::new(shex.body(blob))
        .map_err(|e| format!("output SHEX does not parse: {e}"))?;

    let mut declared = None;
    let mut reads_bias = false;
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("output instruction walk failed: {e}"))?;
        for operand in insn.operands() {
            let operand = operand.map_err(|e| format!("output operand walk failed: {e}"))?;
            if let OperandKind::ConstantBuffer {
                register: 1,
                element,
            } = operand.kind
            {
                if insn.is_declaration() {
                    declared = Some(element);
                } else if element == SSDECAL_EYE_BIAS_REGISTER {
                    reads_bias = true;
                }
            }
        }
    }
    if !reads_bias {
        return Err(format!(
            "no instruction reads cb1[{SSDECAL_EYE_BIAS_REGISTER}]"
        ));
    }
    match declared {
        Some(size) if size > SSDECAL_EYE_BIAS_REGISTER => Ok(()),
        other => Err(format!(
            "cb1 is declared as {other:?} rows, which does not cover register \
             {SSDECAL_EYE_BIAS_REGISTER}",
        )),
    }
}

/// A blob's shader stage, or `None` if it is not a parseable container.
fn stage(blob: &[u8]) -> Option<ShaderStage> {
    Dxbc::parse(blob)
        .ok()
        .and_then(|dxbc| dxbc.shader_chunk())
        .and_then(|shex| TokenStream::new(shex.body(blob)).ok())
        .map(|stream| stream.stage())
}

/// The fixed size of an `ISGN`/`OSGN` element record.
const SIGNATURE_ELEMENT_LEN: usize = 24;

/// One signature element: its semantic name and index, its register, and its component mask.
fn signature_elements(body: &[u8]) -> Vec<(String, u32, u32, u8)> {
    let Some(header) = body.get(..8) else {
        return Vec::new();
    };
    let count = u32::from_le_bytes(header[0..4].try_into().expect("4 bytes")) as usize;
    let table = u32::from_le_bytes(header[4..8].try_into().expect("4 bytes")) as usize;
    (0..count)
        .filter_map(|i| {
            let record = body.get(table + i * SIGNATURE_ELEMENT_LEN..)?;
            let record = record.get(..SIGNATURE_ELEMENT_LEN)?;
            let name_offset =
                u32::from_le_bytes(record[0..4].try_into().expect("4 bytes")) as usize;
            let name = body.get(name_offset..)?.split(|&b| b == 0).next()?;
            Some((
                String::from_utf8_lossy(name).into_owned(),
                u32::from_le_bytes(record[4..8].try_into().expect("4 bytes")),
                u32::from_le_bytes(record[16..20].try_into().expect("4 bytes")),
                record[20],
            ))
        })
        .collect()
}

/// Two elements of one signature that claim the same register with overlapping components, reported
/// as `"<new element> at v<reg> overlaps <existing element>"`.
fn register_collisions(body: &[u8]) -> Vec<String> {
    let mut occupied: BTreeMap<u32, Vec<(String, u8)>> = BTreeMap::new();
    let mut collisions = Vec::new();
    for (name, semantic_index, register, mask) in signature_elements(body) {
        let slot = occupied.entry(register).or_default();
        for (other, other_mask) in slot.iter() {
            if other_mask & mask != 0 {
                collisions.push(format!(
                    "{name}{semantic_index} (mask {mask:#04x}) at v{register} overlaps {other}"
                ));
            }
        }
        slot.push((format!("{name}{semantic_index} (mask {mask:#04x})"), mask));
    }
    collisions
}

/// The rewrite appends `SV_InstanceID` to `ISGN` and `SV_ViewportArrayIndex` to `OSGN` at the next
/// free register, so no two elements of either signature may end up claiming the same register with
/// overlapping components -- a duplicated input register makes the shader's signature contradict
/// itself, and `SV_InstanceID` sharing a register with a vertex attribute is a per-vertex eye index
/// rather than a per-instance one.
///
/// This is the invariant [`corpus_patch_is_sound`] does not cover: it validates the token stream and
/// the `cb0` remap, but never re-reads the signatures it rewrote.
#[test]
fn corpus_patch_signatures_have_no_register_collision() {
    let Some(dir) = shader_dir() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };

    let mut colliding: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        if !is_vertex_shader(&blob) {
            continue;
        }
        let Ok(patched) = patch_vertex_shader(&blob) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let dxbc = Dxbc::parse(&patched).expect("patched container re-parses");
        for tag in [b"ISGN", b"OSGN"] {
            let Some(chunk) = dxbc.chunk(tag) else {
                continue;
            };
            let signature = String::from_utf8_lossy(tag).into_owned();
            for collision in register_collisions(chunk.body(&patched)) {
                colliding.push(format!("{name}: {signature}: {collision}"));
            }
        }
    }

    for line in &colliding {
        eprintln!("  {line}");
    }
    assert!(
        colliding.is_empty(),
        "{} patched shaders have a signature register collision",
        colliding.len()
    );
}

/// SM5 `dcl_input_sgv` / `dcl_output_siv` opcodes, and the system-value names for `SV_InstanceID`
/// and `SV_ViewportArrayIndex`.
const OPCODE_CUSTOMDATA: u32 = 0x35;
const OPCODE_DCL_INPUT_SGV: u32 = 0x60;
const OPCODE_DCL_OUTPUT_SIV: u32 = 0x67;
const SB_NAME_VIEWPORT_ARRAY_INDEX: u32 = 5;
const SB_NAME_INSTANCE_ID: u32 = 8;

/// The registers of the appended `SV_InstanceID` input and `SV_ViewportArrayIndex` output, as
/// *declared in the shader body*. The body is walked as a raw dword array here rather than through
/// the crate's token stream, so the two sides of the comparison share no code.
fn declared_stereo_registers(blob: &[u8]) -> (Option<u32>, Option<u32>) {
    let Some(shex) = Dxbc::parse(blob).ok().and_then(|d| d.shader_chunk()) else {
        return (None, None);
    };
    let tokens: Vec<u32> = shex
        .body(blob)
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
        .collect();

    let (mut input, mut output) = (None, None);
    // Skip the version and length dwords, then walk instruction by instruction: the opcode token's
    // bits 24..31 hold the instruction's length in dwords.
    let mut i = 2;
    while i + 1 < tokens.len() {
        let opcode = tokens[i] & 0x7FF;
        // A custom-data block (an immediate constant buffer) carries no length in its opcode token;
        // its total dword count is the following token. Everything else has it in bits 24..30.
        let length = if opcode == OPCODE_CUSTOMDATA {
            tokens[i + 1] as usize
        } else {
            (tokens[i] >> 24) as usize & 0x7F
        };
        if length == 0 {
            break;
        }
        let end = i + length;
        // `dcl_input_sgv v<reg>.<mask>, <name>` is [opcode, operand, register, name]; the
        // `dcl_output_siv` form is identical.
        if end <= tokens.len() && length == 4 {
            match (opcode, tokens[end - 1]) {
                (OPCODE_DCL_INPUT_SGV, SB_NAME_INSTANCE_ID) => input = Some(tokens[i + 2]),
                (OPCODE_DCL_OUTPUT_SIV, SB_NAME_VIEWPORT_ARRAY_INDEX) => {
                    output = Some(tokens[i + 2])
                }
                _ => {}
            }
        }
        i = end;
    }
    (input, output)
}

/// The register a signature element with the given semantic name occupies, read with this test's own
/// signature reader rather than the rewriter's.
fn signature_element_register(body: &[u8], name: &str) -> Option<u32> {
    signature_elements(body)
        .into_iter()
        .find(|(n, _, _, _)| n == name)
        .map(|(_, _, register, _)| register)
}

/// The rewrite writes each new interface element in two places -- a `dcl_*` in the body and an entry
/// in the signature -- computed from different scans. They must agree: a body that declares
/// `SV_InstanceID` at `v9` while the signature places it at `v8` is a shader whose eye index comes
/// from whatever vertex attribute occupies the register the driver actually binds.
///
/// This is the check that pairs with [`corpus_patch_signatures_have_no_register_collision`]: that one
/// proves the signature is internally consistent, this one proves it describes the body.
#[test]
fn corpus_patch_body_and_signature_agree() {
    let Some(dir) = shader_dir() else {
        return;
    };

    let mut mismatched: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        if !is_vertex_shader(&blob) {
            continue;
        }
        let Ok(patched) = patch_vertex_shader(&blob) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let dxbc = Dxbc::parse(&patched).expect("patched container re-parses");
        let (declared_input, declared_output) = declared_stereo_registers(&patched);
        for (tag, semantic, declared) in [
            (&b"ISGN"[..], "SV_InstanceID", declared_input),
            (&b"OSGN"[..], "SV_ViewportArrayIndex", declared_output),
        ] {
            let signature = dxbc
                .chunk(tag.try_into().expect("4-byte tag"))
                .map(|chunk| signature_element_register(chunk.body(&patched), semantic));
            if signature != Some(declared) {
                mismatched.push(format!(
                    "{name}: {semantic} declared at {declared:?} but the signature says                      {signature:?}"
                ));
            }
        }
    }

    for line in &mismatched {
        eprintln!("  {line}");
    }
    assert!(
        mismatched.is_empty(),
        "{} patched shaders declare a stereo interface element at a register their signature          disagrees with",
        mismatched.len()
    );
}
