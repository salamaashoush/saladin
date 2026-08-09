#!/usr/bin/env python3
"""Driver for the Saladin devctl socket.

    from devctl import Devctl, Build, Train

    with Devctl(port=7777) as g:
        g.cmd(Build(player_id=1, kind="Farm", pos=(12, 30), builders=[peasant]))
        g.step(600)
        farm = g.state(kinds=["buildings"]).buildings_of(1, "Farm")[0]
        assert farm.complete, g.feedback()
        g.screenshot("/tmp/farm.png", camera={"pos": farm.pos, "zoom": 6})

Every write goes out as a PlayerCommand and is applied through lockstep, so a
script drives the game exactly as a player does. `record()` logs the request
stream and `replay()` re-runs it against a fresh match on the same seed,
comparing the state hash at every step: a bug repro that is also a regression
test.

CLI, for a shell pipeline:

    python3 scripts/devctl.py 7777 '{"query": "tick"}'
"""

from __future__ import annotations

import json
import socket
import sys
import time
from typing import Any, Iterable, Sequence

DEFAULT_PORT = 7777
#: One base tick of game time (20 Hz), for hosts that will not be stepped.
TICK_SECONDS = 0.05

# Every PlayerCommand the socket accepts, mirroring devctl::COMMAND_NAMES.
COMMANDS = [
    "Join",
    "AddAi",
    "Move",
    "SetStance",
    "Train",
    "Build",
    "Gather",
    "Attack",
    "SetRally",
    "Garrison",
    "Ungarrison",
    "Demolish",
    "PlaceWall",
    "MarketTrade",
    "MarketBuy",
    "StartResearch",
    "AutoGather",
    "Pause",
    "Resume",
    "Repair",
    "CancelSite",
    "UpgradeBuilding",
    "TrainAt",
    "CancelTrain",
    "GroupMove",
    "AttackMove",
    "GroupAttack",
    "Stop",
    "Embark",
    "Disembark",
]


class DevctlError(RuntimeError):
    """A refused request, a malformed one, or a channel that will not answer."""


class Cmd:
    """One PlayerCommand. Positions may be tuples, lists or Rows' `pos`."""

    def __init__(self, name: str, **fields: Any) -> None:
        if name not in COMMANDS:
            raise DevctlError(f"unknown PlayerCommand: {name}")
        self.name = name
        self.fields = {k: _plain(v) for k, v in fields.items()}

    def json(self) -> dict:
        return {self.name: self.fields}

    def __repr__(self) -> str:
        return f"{self.name}({', '.join(f'{k}={v!r}' for k, v in self.fields.items())})"


def _plain(v: Any) -> Any:
    if isinstance(v, tuple):
        return [_plain(x) for x in v]
    if isinstance(v, list):
        return [_plain(x) for x in v]
    return v


def _factory(name: str):
    def make(**fields: Any) -> Cmd:
        return Cmd(name, **fields)

    make.__name__ = name
    make.__doc__ = f"PlayerCommand::{name}"
    return make


for _n in COMMANDS:
    globals()[_n] = _factory(_n)

__all__ = ["Devctl", "DevctlError", "Cmd", "Row", "State", "wait_for", *COMMANDS]


class Row(dict):
    """A JSON row with attribute access: `farm.complete`, `man.pos`."""

    def __getattr__(self, k: str) -> Any:
        try:
            return self[k]
        except KeyError as e:
            raise AttributeError(k) from e

    @property
    def xy(self) -> tuple[float, float]:
        return (self["pos"][0], self["pos"][1])


class State:
    """One `{"query": "state"}` capture."""

    def __init__(self, raw: dict) -> None:
        self.raw = raw
        self.tick: int = raw.get("tick", 0)
        self.hash: int = raw.get("hash", 0)
        self.seed: int = raw.get("seed", 0)
        self.units = [Row(u) for u in raw.get("units", [])]
        self.buildings = [Row(b) for b in raw.get("buildings", [])]
        self.nodes = [Row(n) for n in raw.get("nodes", [])]
        self.players = [Row(p) for p in raw.get("players", [])]
        self.matches = [Row(m) for m in raw.get("matches", [])]

    def units_of(self, player: int, kind: str | None = None) -> list[Row]:
        return [u for u in self.units if u.owner == player and (kind is None or u.kind == kind)]

    def buildings_of(self, player: int, kind: str | None = None) -> list[Row]:
        return [
            b for b in self.buildings if b.owner == player and (kind is None or b.kind == kind)
        ]

    def nodes_of(self, res: str | None = None) -> list[Row]:
        return [n for n in self.nodes if res is None or n.res == res]

    def player(self, player: int) -> Row:
        for p in self.players:
            if p.player_id == player:
                return p
        raise DevctlError(f"no player {player} in this capture")

    def by_id(self, game_id: int) -> Row | None:
        for row in (*self.units, *self.buildings, *self.nodes):
            if row.get("id") == game_id:
                return row
        return None

    def __repr__(self) -> str:
        return (
            f"State(tick={self.tick} hash={self.hash} units={len(self.units)} "
            f"buildings={len(self.buildings)} nodes={len(self.nodes)})"
        )


class Devctl:
    def __init__(
        self,
        port: int = DEFAULT_PORT,
        host: str = "127.0.0.1",
        timeout: float = 30.0,
    ) -> None:
        self.host = host
        self.port = port
        self.timeout = timeout
        self._sock: socket.socket | None = None
        self._file = None
        self._next_id = 1
        self._log = None

    # ── channel ──────────────────────────────────────────────────────────────

    def _connect(self) -> None:
        self.close()
        try:
            self._sock = socket.create_connection((self.host, self.port), timeout=self.timeout)
        except OSError as e:
            raise DevctlError(f"devctl at {self.host}:{self.port} is not listening: {e}") from e
        self._file = self._sock.makefile("rw", encoding="utf-8", newline="\n")

    def close(self) -> None:
        for h in (self._file, self._sock):
            try:
                if h is not None:
                    h.close()
            except OSError:
                pass
        self._file = self._sock = None

    def __enter__(self) -> "Devctl":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def request(self, req: dict) -> dict:
        """Send one request and return its reply. Reconnects once if the socket
        has gone; a channel that stays dead raises rather than answering with
        an empty result, which would read as 'the game is broken'."""
        for attempt in (0, 1):
            if self._file is None:
                self._connect()
            try:
                return self._exchange(req)
            except (OSError, EOFError) as e:
                self.close()
                if attempt:
                    raise DevctlError(
                        f"devctl at {self.host}:{self.port} dropped the connection: {e}"
                    ) from e
        raise DevctlError("unreachable")

    def _exchange(self, req: dict) -> dict:
        rid = self._next_id
        self._next_id += 1
        out = dict(req, id=rid)
        assert self._file is not None
        self._file.write(json.dumps(out) + "\n")
        self._file.flush()
        while True:
            line = self._file.readline()
            if not line:
                raise EOFError("socket closed")
            reply = json.loads(line)
            # a reply to a request abandoned by an earlier timeout: skip it
            if reply.get("id") == rid:
                if self._log is not None:
                    self._log.write(json.dumps({"req": req, "reply": reply}) + "\n")
                    self._log.flush()
                return reply

    @staticmethod
    def _ok(reply: dict, what: str) -> dict:
        if not reply.get("ok"):
            raise DevctlError(f"{what}: {reply.get('error', reply)}")
        return reply

    # ── verbs ────────────────────────────────────────────────────────────────

    def cmd(self, command: Cmd | dict) -> int:
        """Queue one PlayerCommand. Returns the tick it will be applied on."""
        body = command.json() if isinstance(command, Cmd) else command
        reply = self._ok(self.request({"cmd": body}), f"cmd {body}")
        return reply["applied_tick"]

    def step(self, ticks: int) -> dict:
        """Advance exactly `ticks` base ticks (headless only). Returns the new
        tick and state hash."""
        return self._ok(self.request({"step": int(ticks)}), "step")

    def step_to(self, tick: int, chunk: int = 200) -> dict:
        """Run until the sim has passed `tick` — what `cmd`'s applied_tick is
        for."""
        state = self.tick()
        while state["tick"] < tick:
            state = self.step(min(chunk, tick - state["tick"]))
        return state

    def advance(self, ticks: int, chunk: int = 200) -> dict:
        """Move the match on by `ticks` whatever is hosting it: step if this
        host lets us, else wait the wall-clock equivalent. A client runs on its
        own clock, and a script that assumes otherwise only works headless."""
        info = self.tick()
        if info["may_step"]:
            return self.step_to(info["tick"] + ticks, chunk)
        time.sleep(ticks * TICK_SECONDS)
        return self.tick()

    def tick(self) -> dict:
        return self._ok(self.request({"query": "tick"}), "query tick")

    def state(
        self,
        kinds: Sequence[str] | None = None,
        player: int | None = None,
        near: dict | None = None,
    ) -> State:
        req: dict[str, Any] = {"query": "state"}
        if kinds is not None:
            req["kinds"] = list(kinds)
        if player is not None:
            req["player"] = player
        if near is not None:
            req["near"] = {"pos": _plain(near["pos"]), "radius": near["radius"]}
        return State(self._ok(self.request(req), "query state"))

    def feedback(self) -> list[dict]:
        """Every refusal since the last call — WHY a command did nothing."""
        return self._ok(self.request({"feedback": True}), "feedback")["feedback"]

    def screenshot(self, path: str, camera: dict | None = None) -> str:
        req: dict[str, Any] = {"screenshot": path}
        if camera is not None:
            req["camera"] = {k: _plain(v) for k, v in camera.items()}
        return self._ok(self.request(req), f"screenshot {path}")["path"]

    # ── record / replay ──────────────────────────────────────────────────────

    def record(self, path: str) -> None:
        """Log every request and its reply to `path`. The header carries the
        seed, so a replay can refuse a recording made on a different map."""
        self.stop_recording()
        log = open(path, "w", encoding="utf-8")
        log.write(json.dumps({"devctl_record": 1, "seed": self.tick()["seed"]}) + "\n")
        log.flush()
        self._log = log

    def stop_recording(self) -> None:
        if self._log is not None:
            self._log.close()
            self._log = None

    def replay(self, path: str, strict: bool = True) -> dict:
        """Re-run a recording against THIS game. Only writes and steps are
        replayed — a query changes nothing, so replaying one proves nothing.
        With `strict`, every recorded state hash must come back identical."""
        with open(path, encoding="utf-8") as fh:
            lines = [json.loads(line) for line in fh if line.strip()]
        if not lines or "devctl_record" not in lines[0]:
            raise DevctlError(f"{path} is not a devctl recording")
        want_seed = lines[0]["seed"]
        seed = self.tick()["seed"]
        if seed != want_seed:
            raise DevctlError(f"{path} was recorded on seed {want_seed}, this game is on {seed}")

        last: dict = {}
        for n, entry in enumerate(lines[1:], start=1):
            req = entry.get("req", {})
            if not ({"cmd", "step"} & req.keys()):
                continue
            reply = self._ok(self.request(req), f"replay line {n}: {req}")
            want = entry.get("reply", {})
            if strict and "hash" in want and reply.get("hash") != want["hash"]:
                raise DevctlError(
                    f"replay diverged at line {n} (tick {reply.get('tick')}): "
                    f"hash {reply.get('hash')} != recorded {want['hash']}"
                )
            last = reply
        return last


def wait_for(port: int, host: str = "127.0.0.1", seconds: float = 60.0) -> Devctl:
    """Poll until the game is listening — a runner takes a moment to bind."""
    deadline = time.time() + seconds
    while True:
        try:
            g = Devctl(port=port, host=host)
            g.tick()
            return g
        except DevctlError:
            if time.time() > deadline:
                raise
            time.sleep(0.25)


def _main(argv: Iterable[str]) -> int:
    args = list(argv)
    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    port = int(args[0])
    reqs = args[1:] or [json.dumps({"query": "tick"})]
    with Devctl(port=port) as g:
        for raw in reqs:
            print(json.dumps(g.request(json.loads(raw))))
    return 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
