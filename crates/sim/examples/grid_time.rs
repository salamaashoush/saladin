fn main() {
    let t = std::time::Instant::now();
    let g = saladin_sim::worldgrid::world_grid(saladin_sim::compose_seed(5, 0));
    println!("grid build: {:?} ({} tiles)", t.elapsed(), g.biome.len());
}
