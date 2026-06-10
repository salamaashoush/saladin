//! Saladin relay server (TCP host/join lockstep). Clients join a lobby; the
//! first client hosts and starts the match; then the relay rebroadcasts each
//! tick's complete command batch. Usage: `saladin-server [addr]`
//! (default `127.0.0.1:5000`). Pass `ws <addr>` for the websocket relay
//! (future browser clients).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("ws") {
        let addr = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1:5000".to_string());
        if let Err(e) = saladin_protocol::run_relay_ws(&addr) {
            eprintln!("relay error: {e}");
            std::process::exit(1);
        }
        return;
    }
    let addr = args.first().cloned().unwrap_or_else(|| "127.0.0.1:5000".to_string());
    if let Err(e) = saladin_protocol::run_relay(&addr) {
        eprintln!("relay error: {e}");
        std::process::exit(1);
    }
}
