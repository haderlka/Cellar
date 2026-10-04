//! Regenerate the PNG icons from `assets/logo.svg`:
//!
//! ```text
//! cargo run --example render_icons
//! ```
//!
//! Writes `assets/icon-{32,64,128,256,512,1024}.png`. The 256px icon is
//! embedded as the window icon (see `src/gui/main.rs`).

use resvg::{tiny_skia, usvg};

fn main() {
    let svg = std::fs::read("assets/logo.svg").expect("run from the repository root");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("valid SVG");
    let size = tree.size();
    for px in [32u32, 64, 128, 256, 512, 1024] {
        let mut pixmap = tiny_skia::Pixmap::new(px, px).expect("non-zero size");
        let scale = px as f32 / size.width().max(size.height());
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let path = format!("assets/icon-{}.png", px);
        pixmap.save_png(&path).expect("write PNG");
        println!("wrote {}", path);
    }
}
