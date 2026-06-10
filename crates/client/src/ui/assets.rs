//! Procedurally baked UI art, generated once at startup (client-only, so f32
//! is fine here): aged-parchment panels and bronze buttons as 9-slice images,
//! a pixel-art icon set (resources, units, buildings, techs, stances), bar
//! frames, the menu emblem, selection-ring + rally-flag textures. Everything
//! ships as code — no binary assets, always cohesive, scales cleanly.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::{BorderRect, SliceScaleMode, TextureSlicer};
use saladin_sim::{BuildingKind, Stance, UnitKind};

#[derive(Resource)]
pub struct UiAssets {
    pub panel: Handle<Image>,
    pub panel_dark: Handle<Image>,
    pub button: Handle<Image>,
    pub bar_frame: Handle<Image>,
    pub emblem: Handle<Image>,
    pub backdrop: Handle<Image>,
    pub ring: Handle<Image>,
    pub flag: Handle<Image>,
    icons: HashMap<String, Handle<Image>>,
}

impl UiAssets {
    pub fn icon(&self, key: &str) -> Option<Handle<Image>> {
        self.icons.get(key).cloned()
    }
    pub fn unit_icon(&self, k: UnitKind) -> Option<Handle<Image>> {
        self.icon(&format!("unit:{}", k as u8))
    }
    pub fn building_icon(&self, k: BuildingKind) -> Option<Handle<Image>> {
        self.icon(&format!("bld:{}", k as u8))
    }
    pub fn stance_icon(&self, s: Stance) -> Option<Handle<Image>> {
        self.icon(&format!("stance:{}", s as u8))
    }
    /// 9-slice for panels (12px baked frame).
    pub fn panel_slicer() -> TextureSlicer {
        TextureSlicer {
            border: BorderRect::all(12.0),
            center_scale_mode: SliceScaleMode::Tile { stretch_value: 1.0 },
            sides_scale_mode: SliceScaleMode::Tile { stretch_value: 1.0 },
            ..default()
        }
    }
    /// 9-slice for buttons (4px frame).
    pub fn button_slicer() -> TextureSlicer {
        TextureSlicer { border: BorderRect::all(2.0), ..default() }
    }
    /// 9-slice for thin bar frames (2px).
    pub fn bar_slicer() -> TextureSlicer {
        TextureSlicer { border: BorderRect::all(2.0), ..default() }
    }
}

fn image(w: u32, h: u32, data: Vec<u8>) -> Image {
    Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// Deterministic value noise for texture grain (render-only).
fn grain(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B) ^ seed;
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

fn put(data: &mut [u8], w: u32, x: u32, y: u32, c: [u8; 4]) {
    let i = ((y * w + x) * 4) as usize;
    data[i..i + 4].copy_from_slice(&c);
}

fn shade(c: [u8; 4], f: f32) -> [u8; 4] {
    [
        (c[0] as f32 * f).clamp(0.0, 255.0) as u8,
        (c[1] as f32 * f).clamp(0.0, 255.0) as u8,
        (c[2] as f32 * f).clamp(0.0, 255.0) as u8,
        c[3],
    ]
}

const BRONZE: [u8; 4] = [138, 105, 58, 255];
const BRONZE_DARK: [u8; 4] = [74, 56, 32, 255];
const PARCH: [u8; 4] = [52, 42, 28, 255];
const PARCH_DARK: [u8; 4] = [33, 27, 19, 255];

/// Aged dark parchment with a bronze frame baked into the 12px border.
fn bake_panel(size: u32, base: [u8; 4], seed: u32) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let edge = x.min(y).min(size - 1 - x).min(size - 1 - y);
            let g = grain(x, y, seed) * 0.22 + grain(x / 3, y / 3, seed ^ 7) * 0.18;
            let c = if edge == 0 {
                [12, 9, 6, 255]
            } else if edge <= 2 {
                shade(BRONZE, 0.75 + g)
            } else if edge <= 4 {
                shade(BRONZE_DARK, 0.8 + g)
            } else if edge <= 6 {
                [16, 12, 8, 255]
            } else {
                // mottled parchment fill, slightly darker toward the frame
                let depth = ((edge - 6) as f32 / 10.0).min(1.0);
                shade(base, 0.72 + 0.22 * depth + g)
            };
            put(&mut data, size, x, y, c);
        }
    }
    image(size, size, data)
}

/// Flat bronze button plate: subtle vertical gradient + thin dark outline.
/// Deliberately NOT beveled — flat panels, crisp borders.
fn bake_button(w: u32, h: u32, top: [u8; 4], bottom: [u8; 4], seed: u32) -> Image {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let t = y as f32 / (h - 1) as f32;
        for x in 0..w {
            let g = grain(x, y, seed) * 0.06;
            let c = if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                [16, 12, 8, 255] // thin dark outline
            } else {
                let c = [
                    (top[0] as f32 * (1.0 - t) + bottom[0] as f32 * t) as u8,
                    (top[1] as f32 * (1.0 - t) + bottom[1] as f32 * t) as u8,
                    (top[2] as f32 * (1.0 - t) + bottom[2] as f32 * t) as u8,
                    255,
                ];
                shade(c, 0.97 + g)
            };
            put(&mut data, w, x, y, c);
        }
    }
    image(w, h, data)
}

/// Thin dark inset with a bronze rim — frames HP/morale/progress bars.
fn bake_bar_frame() -> Image {
    let (w, h) = (12u32, 12u32);
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let edge = x.min(y).min(w - 1 - x).min(h - 1 - y);
            let c = match edge {
                0 => shade(BRONZE_DARK, 1.1),
                1 => [10, 8, 6, 255],
                _ => [0, 0, 0, 160],
            };
            put(&mut data, w, x, y, c);
        }
    }
    image(w, h, data)
}

/// Fullscreen menu backdrop: deep leather vignette with faint parchment veins.
fn bake_backdrop(size: u32) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - c) / c;
            let dy = (y as f32 - c) / c;
            let d = (dx * dx + dy * dy).sqrt().min(1.0);
            let g = grain(x, y, 0xBACD) * 0.10 + grain(x / 5, y / 5, 0x77) * 0.12;
            let f = (1.0 - d * 0.65) * (0.65 + g);
            put(&mut data, size, x, y, shade([38, 30, 20, 255], f));
        }
    }
    image(size, size, data)
}

/// Selection ring: a dashed gold circle on transparent (render layer drapes it
/// under selected units/buildings).
fn bake_ring(size: u32) -> Image {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let c = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let r = (dx * dx + dy * dy).sqrt() / c;
            if r > 0.78 && r < 0.97 {
                // 12 dashes around the circle
                let ang = dy.atan2(dx);
                let dash = ((ang / std::f32::consts::PI * 6.0).rem_euclid(1.0) < 0.72) as u8;
                if dash == 1 {
                    let a = (1.0 - ((r - 0.875) / 0.095).abs()).clamp(0.0, 1.0);
                    put(&mut data, size, x, y, [255, 209, 74, (a * 255.0) as u8]);
                }
            }
        }
    }
    image(size, size, data)
}

/// Rally-flag cloth: deep red with a gold crescent.
fn bake_flag(w: u32, h: u32) -> Image {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let g = grain(x, y, 0xF1A6) * 0.18;
            let mut c = shade([148, 38, 24, 255], 0.85 + g);
            // gold crescent: outer circle minus offset inner circle
            let cx = w as f32 * 0.5;
            let cy = h as f32 * 0.5;
            let r = h as f32 * 0.30;
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d_out = (dx * dx + dy * dy).sqrt();
            let d_in = ((dx - r * 0.45) * (dx - r * 0.45) + dy * dy).sqrt();
            if d_out < r && d_in > r * 0.82 {
                c = [255, 209, 74, 255];
            }
            put(&mut data, w, x, y, c);
        }
    }
    image(w, h, data)
}

// ── pixel-art icons ──────────────────────────────────────────────────────────
// 14x14 string art; palette letters map below. '.' = transparent.

fn pal(ch: char) -> Option<[u8; 4]> {
    Some(match ch {
        'k' => [22, 16, 10, 255],    // outline
        'w' => [139, 90, 43, 255],   // wood
        'W' => [176, 122, 64, 255],  // wood light
        's' => [125, 125, 117, 255], // stone
        'S' => [160, 160, 150, 255], // stone light
        'f' => [217, 169, 63, 255],  // wheat
        'F' => [240, 200, 110, 255], // wheat light
        'g' => [255, 209, 74, 255],  // gold
        'G' => [184, 134, 11, 255],  // gold shade
        'm' => [200, 204, 208, 255], // metal
        'M' => [126, 133, 140, 255], // metal shade
        'r' => [160, 51, 38, 255],   // red
        'c' => [232, 220, 192, 255], // cream
        'b' => [90, 122, 170, 255],  // blue
        'e' => [86, 122, 60, 255],   // green
        'E' => [120, 160, 84, 255],  // green light
        'h' => [205, 160, 110, 255], // skin
        _ => return None,
    })
}

fn bake_icon(art: &[&str]) -> Image {
    let h = art.len() as u32;
    let w = art[0].len() as u32;
    let mut data = vec![0u8; (w * h * 4) as usize];
    for (y, row) in art.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if let Some(c) = pal(ch) {
                put(&mut data, w, x as u32, y as u32, c);
            }
        }
    }
    image(w, h, data)
}

#[rustfmt::skip]
const ICONS: &[(&str, [&str; 14])] = &[
    ("res:wood", [
        "..............",
        "..............",
        "..kk......kk..",
        ".kWWkk..kkWWk.",
        ".kWwWWkkWWwWk.",
        ".kWwwWWWWwwWk.",
        "..kWwwwwwwWk..",
        "...kWwwwwWk...",
        "...kWwkkwWk...",
        "...kWwkkwWk...",
        "...kWwwwwWk...",
        "....kkkkkk....",
        "..............",
        "..............",
    ]),
    ("res:stone", [
        "..............",
        "..............",
        "....kkkkk.....",
        "...kSSSSSk....",
        "..kSSssSSSk...",
        ".kSSssssSSSk..",
        ".kSsssssssSk..",
        ".kSsskkssssk..",
        ".kSsskkssssк..",
        ".kssssssssSk..",
        "..kssssssSk...",
        "...kkkkkkk....",
        "..............",
        "..............",
    ]),
    ("res:food", [
        "..............",
        "......kk......",
        ".....kFfk.....",
        "....kFffFk....",
        "....kfFFfk....",
        "...kFffffFk...",
        "...kfFFFFfk...",
        "...kFfffffk...",
        "....kfFFfk....",
        "....kFffk.....",
        ".....kfk......",
        ".....kwk......",
        ".....kwk......",
        "..............",
    ]),
    ("res:gold", [
        "..............",
        "..............",
        "....kkkkk.....",
        "...kggggGk....",
        "..kgGGGGgGk...",
        ".kgGggggGgGk..",
        ".kgGgkkgGgGk..",
        ".kgGgkkgGgGk..",
        ".kgGggggGgGk..",
        "..kgGGGGgGk...",
        "...kggggGk....",
        "....kkkkk.....",
        "..............",
        "..............",
    ]),
    ("unit:0", [
        "..............",
        "...k..k..k....",
        "...kw.kw.kw...",
        "...kw.kw.kw...",
        "...kwkkwkkw...",
        "....kkwwkk....",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kk......",
        "..............",
    ]),
    ("unit:1", [
        "......km......",
        "......kmk.....",
        "......kw......",
        "......kw......",
        "...kkkkw......",
        "..kssskw......",
        ".kssmsskw.....",
        ".ksmsmskw.....",
        ".kssmsskw.....",
        "..kssskw......",
        "...kkkkw......",
        "......kw......",
        "......kk......",
        "..............",
    ]),
    ("unit:2", [
        "..............",
        ".....kkk......",
        "....kwwwk.....",
        "...kw...k.....",
        "..kw....k.....",
        ".kw.....k.....",
        ".kw..mmmk.....",
        ".kw.....k.....",
        "..kw....k.....",
        "...kw...k.....",
        "....kwwwk.....",
        ".....kkk......",
        "..............",
        "..............",
    ]),
    ("unit:3", [
        "..............",
        "....kkkkkk....",
        "...kmmmmmmk...",
        "...kmMMMMmk...",
        "...kmMMMMmk...",
        "...kkkkkkkk...",
        "...kmkmmkmk...",
        "...kmkmmkmk...",
        "...kmmmmmmk...",
        "...kmMmmMmk...",
        "...kmmmmmmk...",
        "....kkkkkk....",
        "..............",
        "..............",
    ]),
    ("unit:4", [
        "..............",
        ".....kk.......",
        "....kWWk......",
        "...kWWWWk.....",
        "...kWkWWk.....",
        "...kWWWWWk....",
        "....kWWWWWk...",
        ".....kWWWWk...",
        ".....kWWk.....",
        ".....kWWk..k..",
        ".....kWWk.kmk.",
        ".....kkkk..k..",
        "..............",
        "..............",
    ]),
    ("unit:5", [
        "..............",
        ".........kk...",
        ".......kkmgk..",
        "......kmmgk...",
        ".....kmmgk....",
        "....kmmgk.....",
        "....kmgk......",
        "...kmgk.......",
        "...kgk........",
        "..kGGk........",
        "..kwwk........",
        "..kkkk........",
        "..............",
        "..............",
    ]),
    ("unit:6", [
        "..............",
        "......kk......",
        "...kkkmmkkk...",
        "..kmmmmmmmmk..",
        "...kkkmmkkk...",
        "......kw......",
        "......kw......",
        "....kkkwkk....",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kk......",
        "..............",
        "..............",
    ]),
    ("unit:7", [
        "..............",
        "..............",
        "..............",
        "...kkkkkkkkk..",
        "..kWWWWWWWWWk.",
        ".kmMWwwwwwwWk.",
        ".kmMWwwwwwwWk.",
        "..kWWWWWWWWWk.",
        "...kkkkkkkkk..",
        "....kk...kk...",
        "....kk...kk...",
        "..............",
        "..............",
        "..............",
    ]),
    ("unit:8", [
        "..............",
        "..........kk..",
        ".........kwk..",
        "........kwk...",
        ".......kwk....",
        "......kwk.....",
        ".....kwk......",
        "..kkkwwkkk....",
        ".kwwwwwwwwk...",
        ".kwkkwwkkwk...",
        "..kk.kk.kk....",
        "..kk.kk.kk....",
        "..............",
        "..............",
    ]),
    ("unit:9", [
        "..............",
        ".....kggk.....",
        "....kgkkgk....",
        "....kgk.......",
        "....kgk.......",
        "....kgkkgk....",
        ".....kggk.....",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kw......",
        "......kk......",
        "..............",
        "..............",
    ]),
    ("bld:0", [
        "..............",
        "..k.k..k.k....",
        "..kskksksk....",
        "..kssssssk....",
        "..ksSSSSsk....",
        "..ksSssSsk....",
        "..ksSssSsk....",
        ".kssssssssk...",
        ".ksSSSSSSsk...",
        ".ksSskksSsk...",
        ".ksSskksSsk...",
        ".kkkkkkkkkk...",
        "..............",
        "..............",
    ]),
    ("bld:1", [
        "..............",
        "..............",
        "......kk......",
        ".....kcck.....",
        "....kcccck....",
        "...kccrccck...",
        "..kccrrrcck...",
        ".kccrrrrrcck..",
        ".kcrrkkrrrck..",
        "kccrrkkrrrcck.",
        "kkkkkkkkkkkkk.",
        "..............",
        "..............",
        "..............",
    ]),
    ("bld:2", [
        "..............",
        "...k.kk.k.....",
        "...kksskk.....",
        "...kssssk.....",
        "...ksSSsk.....",
        "...ksSSsk.....",
        "...kssssk.....",
        "...ksSSsk.....",
        "...ksSSsk.....",
        "...kskksk.....",
        "...kskksk.....",
        "...kkkkkk.....",
        "..............",
        "..............",
    ]),
    ("bld:3", [
        "..............",
        "..............",
        "..k.k.k.k.k...",
        "..kkkkkkkkk...",
        "..kssSssSsk...",
        "..kSskSskSk...",
        "..kssSssSsk...",
        "..kSskSskSk...",
        "..kssSssSsk...",
        "..kkkkkkkkk...",
        "..............",
        "..............",
        "..............",
        "..............",
    ]),
    ("bld:4", [
        "..............",
        "..k.k.kk.k.k..",
        "..kkkkkkkkkk..",
        "..kssssssssk..",
        "..ksSSkkSSsk..",
        "..ksSkwwkSsk..",
        "..ksSkwwkSsk..",
        "..ksSkwwkSsk..",
        "..ksSkwwkSsk..",
        "..ksSkwwkSsk..",
        "..kkkkkkkkkk..",
        "..............",
        "..............",
        "..............",
    ]),
    ("bld:5", [
        "..............",
        "..............",
        "......kk......",
        ".....kwwk.....",
        "....kwwwwk....",
        "...kwwwwwwk...",
        "..kwwwwwwwwk..",
        "..kkkkkkkkkk..",
        "..kcckkkkcck..",
        "..kcckwwkcck..",
        "..kcckwwkcck..",
        "..kkkkkkkkkk..",
        "..............",
        "..............",
    ]),
    ("bld:6", [
        "..............",
        "..............",
        "....kkkkkk....",
        "...kmmmmmmk...",
        "..kmmkkkkmmk..",
        "..kmk....kmk..",
        "..kmk....kmk..",
        "..kmk....kmk..",
        "..kmk....kmk..",
        "..kmk....kmk..",
        "..kkk....kkk..",
        "..............",
        "..............",
        "..............",
    ]),
    ("bld:7", [
        "..............",
        "..............",
        "..............",
        "..kkkkkkkkk...",
        ".kmmmmmmmmmk..",
        ".kkkmmmmmkkk..",
        "...kmmmmmk....",
        "....kmmmk.....",
        "....kmmmk.....",
        "...kmmmmmk....",
        "..kmmmmmmmk...",
        "..kkkkkkkkk...",
        "..............",
        "..............",
    ]),
    ("bld:8", [
        "..............",
        "......kk......",
        "..kkkkggkkkk..",
        ".kgk..gg..kgk.",
        ".kgk..gg..kgk.",
        "kgggk.gg.kgggk",
        ".kkk..gg..kkk.",
        "......gg......",
        "......gg......",
        "....kkggkk....",
        "...kggggggk...",
        "...kkkkkkkk...",
        "..............",
        "..............",
    ]),
    ("bld:9", [
        "..............",
        "..............",
        ".....kkkk.....",
        "....kwFwk.....",
        "....kkkkk.....",
        "...kFFFFFk....",
        "..kFFfFfFFk...",
        "..kFfFFFfFk...",
        "..kFFfFfFFk...",
        "..kFfFFFfFk...",
        "...kFFFFFk....",
        "....kkkkk.....",
        "..............",
        "..............",
    ]),
    ("bld:10", [
        "..............",
        "..............",
        "..............",
        "....kkkk......",
        "..kkbbbbkk..k.",
        ".kbbSbbbbbkkk.",
        "kbbbbbbbbbbbk.",
        ".kbbbbbbbbkkk.",
        "..kkbbbbkk..k.",
        "....kkkk......",
        "..............",
        "..............",
        "..............",
        "..............",
    ]),
    ("bld:11", [
        "..............",
        "....kkkkkk....",
        "...kwwkkwwk...",
        "..kwk.kk.kwk..",
        ".kwk..kk..kwk.",
        ".kwkkkkkkkkwk.",
        ".kwkkkkkkkkwk.",
        ".kwk..kk..kwk.",
        "..kwk.kk.kwk..",
        "...kwwkkwwk...",
        "....kkkkkk....",
        "..............",
        "..............",
        "..............",
    ]),
    ("bld:12", [
        "......kr......",
        ".....krrk.....",
        "......kr......",
        "...kkkkkkk....",
        "...kw...wk....",
        "....kwwwk.....",
        "....kwwwk.....",
        "....kwwwk.....",
        "....kwwwk.....",
        "...kwwwwwk....",
        "...kwwwwwk....",
        "...kkkkkkk....",
        "..............",
        "..............",
    ]),
    ("stance:0", [
        "..............",
        ".km........mk.",
        ".kmm......mmk.",
        "..kmm....mmk..",
        "...kmm..mmk...",
        "....kmmmmk....",
        ".....kmmk.....",
        ".....kmmk.....",
        "....kmmmmk....",
        "...kmmkkmmk...",
        "..kGGk..kGGk..",
        "..kkk....kkk..",
        "..............",
        "..............",
    ]),
    ("stance:1", [
        "..............",
        "..kkkkkkkkkk..",
        ".kmSSSSSSSSk..",
        ".kmSbbbbbbSk..",
        ".kmSbSSSSbSk..",
        ".kmSbSbbSbSk..",
        ".kmSbSbbSbSk..",
        ".kmSbSSSSbSk..",
        "..kmSbbbbSk...",
        "...kmSSSSk....",
        "....kmSSk.....",
        ".....kkk......",
        "..............",
        "..............",
    ]),
    ("stance:2", [
        "..............",
        "...kw.........",
        "...kwrrrrrk...",
        "...kwrrrrrrk..",
        "...kwrrrrrk...",
        "...kwrrrrk....",
        "...kw.........",
        "...kw.........",
        "...kw.........",
        "...kw.........",
        "...kw.........",
        "...kk.........",
        "..............",
        "..............",
    ]),
    ("act:demolish", [
        "..............",
        ".kk........kk.",
        ".krk......krk.",
        "..krk....krk..",
        "...krk..krk...",
        "....krkkrk....",
        ".....krrk.....",
        ".....krrk.....",
        "....krkkrk....",
        "...krk..krk...",
        "..krk....krk..",
        ".krk......krk.",
        ".kk........kk.",
        "..............",
    ]),
    ("tech:scroll", [
        "..............",
        "...kkkkkkkk...",
        "..kcccccccck..",
        "..kcGGGGGGck..",
        "..kcccccccck..",
        "..kcGGGGccck..",
        "..kcccccccck..",
        "..kcGGGGGcck..",
        "..kcccccccck..",
        "...kkkkkkkk...",
        ".....kcck.....",
        "......kk......",
        "..............",
        "..............",
    ]),
];

pub fn build(images: &mut Assets<Image>) -> UiAssets {
    let mut icons = HashMap::new();
    for (key, art) in ICONS {
        icons.insert(key.to_string(), images.add(bake_icon(art)));
    }
    UiAssets {
        panel: images.add(bake_panel(64, PARCH, 0x51AD)),
        panel_dark: images.add(bake_panel(64, PARCH_DARK, 0x77E1)),
        button: images.add(bake_button(48, 32, [128, 99, 58, 255], [96, 72, 42, 255], 0xB7)),
        bar_frame: images.add(bake_bar_frame()),
        emblem: images.add(bake_flag(48, 48)),
        backdrop: images.add(bake_backdrop(256)),
        ring: images.add(bake_ring(64)),
        flag: images.add(bake_flag(20, 14)),
        icons,
    }
}

/// Marker for the HUD's minimap frame node (positioned over the minimap
/// camera viewport each frame by `minimap::update_minimap_viewport`).
#[derive(Component)]
pub struct MinimapFrame;
