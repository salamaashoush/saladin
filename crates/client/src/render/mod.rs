//! Render layer: procedural model meshes, instanced drawing of sim entities,
//! LOD, and visual effects. Pure render — never writes sim state.

pub mod ghost;
pub mod models;
pub mod sync;
pub mod terrain_material;
