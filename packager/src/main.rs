use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("missing parent")?
        .to_path_buf();
    let core_wasm =
        root.join("plugin/target/wasm32-unknown-unknown/release/solidcraft_luer_fitting_plugin.wasm");
    let wit_path = root.join("wit/plugin.wit");
    let manifest_path = root.join("plugin.json");

    let mut wasm_bytes =
        fs::read(&core_wasm).with_context(|| format!("read {}", core_wasm.display()))?;
    let mut resolve = Resolve::default();
    let (pkg_id, _) = resolve
        .push_path(&wit_path)
        .with_context(|| format!("parse {}", wit_path.display()))?;
    let world_id = resolve.select_world(&[pkg_id], Some("command-dialog-plugin"))?;

    wit_component::embed_component_metadata(&mut wasm_bytes, &resolve, world_id, StringEncoding::UTF8)?;
    let component = ComponentEncoder::default()
        .module(&wasm_bytes)?
        .validate(true)
        .encode()?;

    let dist_dir = root.join("dist/solidcraft-luer-fitting");
    fs::create_dir_all(&dist_dir)?;
    let out_wasm = dist_dir.join("solidcraft-luer-fitting.wasm");
    let out_manifest = dist_dir.join("plugin.json");
    let manifest_bytes = fs::read(&manifest_path)?;

    fs::write(&out_wasm, &component)?;
    fs::write(&out_manifest, &manifest_bytes)?;

    let zip_path = root.join("dist/solidcraft-luer-fitting.zip");
    let file = File::create(&zip_path)?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("solidcraft-luer-fitting/plugin.json", opts)?;
    zip.write_all(&manifest_bytes)?;
    zip.start_file("solidcraft-luer-fitting/solidcraft-luer-fitting.wasm", opts)?;
    zip.write_all(&component)?;
    zip.finish()?;

    println!("Built component: {} ({} bytes)", out_wasm.display(), component.len());
    println!("Packaged zip:    {}", zip_path.display());
    Ok(())
}
