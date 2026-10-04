//! Regenerate the icons from `assets/logo.svg`:
//!
//! ```text
//! cargo run --example render_icons
//! ```
//!
//! Writes `assets/icon-{32,64,128,256,512,1024}.png` and `assets/icon.ico`.
//! The 256px icon is embedded as the window icon (see `src/gui/main.rs`),
//! the `.ico` as the icon of `cellar.exe` (see `build.rs`).

use resvg::{tiny_skia, usvg};

fn main() {
    let svg = std::fs::read("assets/logo.svg").expect("run from the repository root");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("valid SVG");
    let render = |px: u32| {
        let size = tree.size();
        let mut pixmap = tiny_skia::Pixmap::new(px, px).expect("non-zero size");
        let scale = px as f32 / size.width().max(size.height());
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        pixmap
    };

    for px in [32u32, 64, 128, 256, 512, 1024] {
        let path = format!("assets/icon-{}.png", px);
        render(px).save_png(&path).expect("write PNG");
        println!("wrote {}", path);
    }

    // Windows icon: the sizes Explorer and the taskbar ask for, each stored
    // as PNG (supported since Windows Vista).
    let pngs: Vec<(u32, Vec<u8>)> = [16u32, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|px| (px, render(px).encode_png().expect("encode PNG")))
        .collect();
    let path = "assets/icon.ico";
    std::fs::write(path, ico(&pngs)).expect("write ICO");
    println!("wrote {}", path);
}

/// An ICO file holding the given `(size, PNG bytes)` images.
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (px, png) in images {
        let dim = if *px >= 256 { 0 } else { *px as u8 }; // 0 means 256
        out.extend_from_slice(&[dim, dim, 0, 0]); // width, height, palette, reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}
