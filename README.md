# Saladin

A historic real-time strategy game (Crusades era) written in Rust on
[Bevy](https://bevy.org), with deterministic-lockstep multiplayer over TCP.

The repo root is the cargo workspace:

- `crates/sim` — pure deterministic game core (fixed-point math, worldgen,
  pathfinding, combat/economy/AI formulas)
- `crates/protocol` — the simulation as Bevy ECS + lockstep netcode + save/load
- `crates/server` — dedicated relay binary (optional; clients can host)
- `crates/client` — the game: rendering, camera, input, UI

```bash
cargo test --workspace
cargo run -p saladin-client --bin saladin-client            # play vs AI
cargo run -p saladin-client --bin saladin-client connect <ip>  # join a LAN game
```

To host a multiplayer game, click **Host Game (LAN)** in the menu; friends
join with `connect <your-ip>`. See `CLAUDE.md` for architecture notes.
