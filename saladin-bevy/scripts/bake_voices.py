# /// script
# requires-python = ">=3.11,<3.13"
# dependencies = [
#   "chatterbox-tts",
#   "librosa",
#   "soundfile",
#   "numpy",
# ]
# [tool.uv]
# # chatterbox pins an older torch whose wheels lack Blackwell (sm_120)
# # kernels — force one that has them.
# override-dependencies = ["torch>=2.9", "torchaudio>=2.9"]
# ///
"""Bake unit voice barks with Chatterbox multilingual TTS (Arabic).

Output: saladin-bevy/assets/voices/{kind}_{bark}_{variant}.wav
        (22.05 kHz mono 16-bit — what the engine's loader expects)

The engine falls back to its procedural formant voices for any file that is
missing, so partial bakes are fine. Ram/Mangonel keep their procedural creak
on purpose — siege engines don't talk.

Run:  uv run scripts/bake_voices.py            # full bake
      uv run scripts/bake_voices.py peasant    # one kind only
"""

import sys
from pathlib import Path

import librosa
import numpy as np
import soundfile as sf

OUT = Path(__file__).resolve().parent.parent / "assets" / "voices"
SR_OUT = 22050

# (kind, pitch shift in semitones, speed) — the same registers the
# procedural voices use: peasants high and quick, knights low and heavy.
KINDS = {
    "peasant": (3.0, 1.06),
    "spearman": (0.0, 1.0),
    "archer": (2.0, 1.04),
    "knight": (-3.0, 0.94),
    "horsearcher": (1.5, 1.08),
    "mamluk": (-1.5, 0.97),
    "crossbowman": (1.0, 1.0),
    "imam": (-1.0, 0.90),
}

# bark -> two Arabic variants. Kept short: these are battle barks, not lines.
BARKS = {
    "ack": ["نعم", "حاضر"],
    "attack": ["يلا! هجوم!", "هجوم!"],
    "wood": ["حطب", "نجمع الحطب"],
    "food": ["طعام", "نجمع الطعام"],
    "stone": ["حجر", "نجمع الحجارة"],
    "gold": ["ذهب", "نجمع الذهب"],
}

# the imam answers everything the same way
IMAM_LINES = ["إن شاء الله", "بسم الله"]


def main() -> None:
    only = sys.argv[1] if len(sys.argv) > 1 else None
    OUT.mkdir(parents=True, exist_ok=True)

    import torch
    from chatterbox.mtl_tts import ChatterboxMultilingualTTS

    import os

    device = os.environ.get("TTS_DEVICE") or ("cuda" if torch.cuda.is_available() else "cpu")
    print(f"loading Chatterbox multilingual on {device}…")
    model = ChatterboxMultilingualTTS.from_pretrained(device=device)

    def synth(text: str, exaggeration: float) -> np.ndarray:
        wav = model.generate(
            text,
            language_id="ar",
            exaggeration=exaggeration,
            cfg_weight=0.4,
        )
        return wav.squeeze(0).cpu().numpy()

    for kind, (semitones, speed) in KINDS.items():
        if only and kind != only:
            continue
        for bark, lines in BARKS.items():
            lines = IMAM_LINES if kind == "imam" else lines
            for variant, text in enumerate(lines):
                path = OUT / f"{kind}_{bark}_{variant}.wav"
                if path.exists():
                    print(f"skip {path.name}")
                    continue
                # attack barks get shouted
                exaggeration = 0.9 if bark == "attack" else 0.45
                wav = synth(text, exaggeration)
                sr = model.sr
                # per-kind register: pitch shift + tempo, then resample down
                wav = librosa.effects.pitch_shift(y=wav, sr=sr, n_steps=semitones)
                if speed != 1.0:
                    wav = librosa.effects.time_stretch(y=wav, rate=speed)
                wav = librosa.resample(y=wav, orig_sr=sr, target_sr=SR_OUT)
                wav = librosa.util.normalize(wav) * 0.8
                # trim leading/trailing silence so barks feel snappy
                wav, _ = librosa.effects.trim(wav, top_db=35)
                sf.write(path, wav, SR_OUT, subtype="PCM_16")
                print(f"baked {path.name}  ({len(wav) / SR_OUT:.2f}s)  '{text}'")

    print("done.")


if __name__ == "__main__":
    main()
