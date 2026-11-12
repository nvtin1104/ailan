use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "src/platform/icon_art.rs"]
mod icon_art;

fn main() {
    println!("cargo:rerun-if-changed=src/platform/icon_art.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ico_path = out_dir.join("ailan.ico");
    let rc_path = out_dir.join("ailan.rc");

    // Render multi-size ICO
    let ico_bytes = icon_art::render_ico_bytes(&[16, 20, 24, 32, 40, 48, 64, 128, 256]);
    fs::write(&ico_path, ico_bytes).expect("Failed to write ailan.ico");

    let rc_content = format!(
        "1 ICON \"{}\"\n",
        ico_path.to_str().unwrap().replace('\\', "/")
    );
    fs::write(&rc_path, rc_content).expect("Failed to write ailan.rc");

    let _ = embed_resource::compile(&rc_path, embed_resource::NONE);
}
