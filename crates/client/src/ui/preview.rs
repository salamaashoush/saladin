//! Minimap PREVIEW for the skirmish setup and the multiplayer lobby: the
//! chosen seed+preset rendered into a small biome-colored image (the same
//! terrain sampling the sim authorities over), with gold start markers.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use saladin_sim::{Fx, WORLD_SIZE, biome_def, fx, sample_terrain, start_point};

pub const PREVIEW_PX: u32 = 144;

/// seed → generated preview image (tiny RGBA, fine to keep for a session).
#[derive(Resource, Default)]
pub struct PreviewCache(pub HashMap<u32, Handle<Image>>);

pub fn preview_handle(
    cache: &mut PreviewCache,
    images: &mut Assets<Image>,
    seed: u32,
) -> Handle<Image> {
    if let Some(h) = cache.0.get(&seed) {
        return h.clone();
    }
    let h = images.add(render_preview(seed));
    cache.0.insert(seed, h.clone());
    h
}

fn render_preview(seed: u32) -> Image {
    let px = PREVIEW_PX as usize;
    let mut data = vec![0u8; px * px * 4];
    let half = fx!("0.5");
    for py in 0..px {
        let ty = (py as i32 * WORLD_SIZE) / px as i32;
        for pxx in 0..px {
            let tx = (pxx as i32 * WORLD_SIZE) / px as i32;
            let s = sample_terrain(seed, Fx::from_num(tx) + half, Fx::from_num(ty) + half);
            let c = biome_def(s.biome).color;
            let i = (py * px + pxx) * 4;
            data[i] = (c >> 16) as u8;
            data[i + 1] = (c >> 8) as u8;
            data[i + 2] = c as u8;
            data[i + 3] = 255;
        }
    }
    // gold start markers (all 8 slots)
    for slot in 0..8 {
        let p = start_point(seed, slot);
        let mx = (p.x.to_num::<i32>() * px as i32 / WORLD_SIZE).clamp(1, px as i32 - 2);
        let my = (p.y.to_num::<i32>() * px as i32 / WORLD_SIZE).clamp(1, px as i32 - 2);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let i = (((my + dy) as usize) * px + (mx + dx) as usize) * 4;
                data[i] = 255;
                data[i + 1] = 209;
                data[i + 2] = 74;
            }
        }
    }
    Image::new(
        Extent3d { width: PREVIEW_PX, height: PREVIEW_PX, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// UI node: framed preview image.
pub fn preview_node(p: &mut ChildSpawnerCommands, handle: Handle<Image>) {
    p.spawn((
        Node {
            width: Val::Px(PREVIEW_PX as f32),
            height: Val::Px(PREVIEW_PX as f32),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(super::theme::PANEL_BORDER),
        ImageNode::new(handle),
    ));
}
