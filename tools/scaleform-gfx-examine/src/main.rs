use std::path::PathBuf;

mod gfx;
mod tags;

use gfx::GfxFile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.gfx> [output_dir]", args[0]);
        eprintln!();
        eprintln!("Parses a Scaleform GFX file and dumps its structure:");
        eprintln!("  - Tag inventory (type, size, offset)");
        eprintln!("  - SymbolClass mappings (tag ID -> AS3 class name)");
        eprintln!("  - ExportAssets (exported symbol names)");
        eprintln!("  - DoABC constant pools (class/method/string names)");
        eprintln!("  - DefineSprite depth tables (z-layer per element)");
        eprintln!("  - PlaceObject depth placements");
        eprintln!();
        eprintln!("If output_dir is given, also dumps raw tag data and ABC blocks to files.");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let output_dir = args.get(2).map(PathBuf::from);

    let data = match std::fs::read(&input_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error reading {}: {e}", input_path.display());
            std::process::exit(1);
        }
    };

    let gfx = match GfxFile::parse(&data) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error parsing {}: {e}", input_path.display());
            std::process::exit(1);
        }
    };

    println!("=== {} ===", input_path.display());
    println!("magic:       {}", gfx.magic);
    println!("version:     {}", gfx.version);
    println!("file_length: {} (uncompressed)", gfx.file_length);
    println!("on_disk:     {} bytes", data.len());
    println!("body_length: {} bytes (decompressed)", gfx.body.len());
    println!();

    // Tag inventory
    println!("=== Tags ({}) ===", gfx.tags.len());
    let mut tag_counts: std::collections::HashMap<u16, (usize, usize)> =
        std::collections::HashMap::new();
    for tag in &gfx.tags {
        let entry = tag_counts.entry(tag.code).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += tag.data.len();
    }
    let mut sorted_tags: Vec<_> = tag_counts.iter().collect();
    sorted_tags.sort_by_key(|(code, _)| **code);
    for (code, (count, total_size)) in &sorted_tags {
        let name = tags::tag_name(**code);
        println!(
            "  {:3} {:30} x{:3}  {:>10} bytes",
            code, name, count, total_size
        );
    }
    println!();

    // Detailed dump
    if let Err(e) = tags::dump(&gfx, &output_dir) {
        eprintln!("warning: error during tag dump: {e}");
    }

    // Dump raw data if requested
    if let Some(ref out_dir) = output_dir {
        if let Err(e) = std::fs::create_dir_all(out_dir) {
            eprintln!("warning: could not create output dir: {e}");
        } else {
            for (i, tag) in gfx.tags.iter().enumerate() {
                if tag.data.is_empty() {
                    continue;
                }
                let name = tags::tag_name(tag.code);
                let fname = format!("{:03}-{}-{}.bin", i, tag.code, name);
                let path = out_dir.join(&fname);
                let _ = std::fs::write(&path, &tag.data);
            }
            println!("Raw tag data dumped to: {}", out_dir.display());
        }
    }
}
