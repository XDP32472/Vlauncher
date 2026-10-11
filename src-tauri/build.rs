use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let icons = manifest.join("icons");
    let icon = icons.join("icon.ico");

    // Keep the app buildable before artwork is added: emit a small valid Vlauncher
    // icon (dark background and a lime four-square mark) for Windows packaging.
    if !icon.exists() {
        fs::create_dir_all(&icons).expect("create icon directory");
        fs::write(&icon, make_icon()).expect("write Windows icon");
    }
    println!("cargo:rerun-if-changed={}", icon.display());
    tauri_build::build()
}

fn make_icon() -> Vec<u8> {
    const SIZE: usize = 32;
    const PIXEL_BYTES: usize = SIZE * SIZE * 4;
    const MASK_BYTES: usize = SIZE * 4;
    const IMAGE_BYTES: usize = 40 + PIXEL_BYTES + MASK_BYTES;
    let mut out = Vec::with_capacity(22 + IMAGE_BYTES);

    // ICO directory and one 32-bit DIB image.
    out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    out.extend_from_slice(&[SIZE as u8, SIZE as u8, 0, 0]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&(IMAGE_BYTES as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());

    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(SIZE as i32).to_le_bytes());
    out.extend_from_slice(&((SIZE * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(PIXEL_BYTES as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..SIZE).rev() {
        for x in 0..SIZE {
            let mark = (8..24).contains(&x)
                && (8..24).contains(&y)
                && ((x < 15) == (y < 15));
            let (b, g, r) = if mark { (107, 243, 198) } else { (15, 17, 16) };
            out.extend_from_slice(&[b, g, r, 255]);
        }
    }
    out.resize(out.len() + MASK_BYTES, 0);
    out
}
