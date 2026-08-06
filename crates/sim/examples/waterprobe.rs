// Scratch probe: where the sea surface sits, and what a "fishery" node lands on.
// Replicates the client's bilinear HeightField exactly (client/src/terrain.rs).
fn main() {
    use saladin_sim::*;
    use saladin_sim::terrain::ScatterDomain;
    let seed = compose_seed(1000, 3);
    let gain = seed_bias(seed).elev_gain;

    // client HeightField: half-tile lattice of surface_height, bilinear read
    let m = (WORLD_SIZE * 2 + 1) as usize;
    let mut h = Vec::with_capacity(m * m);
    for iy in 0..m {
        for ix in 0..m {
            let s = sample_terrain(seed, Fx::from_num(ix as f32 * 0.5), Fx::from_num(iy as f32 * 0.5));
            h.push(surface_height(s.height, gain).to_num::<f32>());
        }
    }
    let height_at = |x: f32, z: f32| -> f32 {
        let n = m as i32;
        let fx = (x * 2.0).clamp(0.0, (n - 1) as f32);
        let fz = (z * 2.0).clamp(0.0, (n - 1) as f32);
        let (x0, z0) = (fx.floor() as i32, fz.floor() as i32);
        let (x1, z1) = ((x0 + 1).min(n - 1), (z0 + 1).min(n - 1));
        let (tx, tz) = (fx - x0 as f32, fz - z0 as f32);
        let s = |gx: i32, gz: i32| h[(gz * n + gx) as usize];
        let a = s(x0, z0) * (1.0 - tx) + s(x1, z0) * tx;
        let b = s(x0, z1) * (1.0 - tx) + s(x1, z1) * tx;
        a * (1.0 - tz) + b * tz
    };

    let fish_rule: Vec<_> = content::node_kinds().into_iter().filter(|r| r.domain == ScatterDomain::Water).collect();
    let nodes = scatter_nodes(seed, &fish_rule);
    let (mut fishy, mut landy) = (0, 0);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for nd in &nodes {
        let g = height_at(nd.pos.x.to_num::<f32>(), nd.pos.y.to_num::<f32>());
        lo = lo.min(g);
        hi = hi.max(g);
        if g < -0.005 { fishy += 1 } else { landy += 1 }
    }
    println!("{} fishery nodes, seed 1000 preset 3", nodes.len());
    println!("  height_at range {lo:.4} .. {hi:.4}");
    println!("  drawn as FISH SCHOOL: {fishy}");
    println!("  drawn as DEER/BOAR/BERRY on the beach: {landy}");

    // and across a spread of seeds/presets, per preset
    println!("\npreset  fish  land   (point sample, 5 seeds each)");
    for preset in 0..4u8 {
        let (mut tf, mut tl) = (0, 0);
        for base in [11u32, 48514, 1000, 7, 99] {
            let s = compose_seed(base, preset);
            let gain = seed_bias(s).elev_gain;
            let r: Vec<_> = content::node_kinds().into_iter().filter(|r| r.domain == ScatterDomain::Water).collect();
            for nd in scatter_nodes(s, &r) {
                let t = sample_terrain(s, nd.pos.x, nd.pos.y);
                let g = surface_height(t.height, gain).to_num::<f32>();
                if g < -0.005 { tf += 1 } else { tl += 1 }
            }
        }
        println!("  {preset}    {tf:>5} {tl:>5}");
    }
}
