# saladin-server — internet relay

A tiny TCP relay hosting many concurrent ROOMS. Lockstep ships player inputs
only, so bandwidth is a few hundred bytes per player per second — the smallest
VPS tier anywhere runs it. Both the host and the joiners connect OUTBOUND to
the relay, so it traverses NAT with zero router configuration on either side.

## Deploy on a VPS

```bash
# build a release binary (on the VPS, or cross-compile and scp it)
cargo build --release -p saladin-server
# run it on a public port (default 5000)
./target/release/saladin-server 0.0.0.0:5000
```

Open TCP port 5000 in the VPS firewall. That is the entire setup.

Optional systemd unit (`/etc/systemd/system/saladin-relay.service`):

```ini
[Unit]
Description=Saladin lockstep relay
After=network.target

[Service]
ExecStart=/opt/saladin/saladin-server 0.0.0.0:5000
Restart=always
User=nobody

[Install]
WantedBy=multi-user.target
```

## Point clients at it

Every player edits `~/.config/saladin/config.toml`:

```toml
relay_addr = "your.vps.example.com:5000"
```

In-game: one player picks **Multiplayer → Host Internet Game**, reads the
6-character room code to friends; they pick **Join Room** and type it. Players
on mismatched game builds are rejected with a clear version-mismatch message
(`PROTOCOL_VERSION` handshake).

The relay keeps no persistent state and simulates nothing — it assigns seats,
relays lobby changes, and re-broadcasts each tick's complete command batch.
Rooms are deleted when their last human disconnects.
