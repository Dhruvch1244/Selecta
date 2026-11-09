# Selecta stem worker. Runs inside Selecta's private Python env (see stems.rs).
#
# One long-lived process: the model loads once (2-5 s), then each job is a
# JSON line on stdin, {"id", "path", "out"}, answered by one JSON line on
# stdout. Only the vocal stem is written: the app derives the instrumental as
# (original - vocals) at load time, so the full mix stays the untouched
# original and the files on disk stay small.
#
# Besides the audio it returns the vocal loudness envelope (10 frames/s, dB),
# which the planner uses to find acapella phrases and avoid vocal clashes.

import json
import math
import os
import sys

import numpy as np
import soundfile as sf
import torch
from demucs.apply import apply_model
from demucs.audio import AudioFile
from demucs.pretrained import get_model


def say(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def write_ogg(path, data, rate):
    # libsndfile's Vorbis encoder segfaults on one large write (seen with a
    # 3-minute track); feeding it in small blocks is the known workaround.
    tmp = path + ".part"
    with sf.SoundFile(tmp, "w", rate, data.shape[1], format="OGG", subtype="VORBIS") as f:
        for i in range(0, len(data), 8192):
            f.write(np.ascontiguousarray(data[i : i + 8192], dtype=np.float32))
    os.replace(tmp, path)  # never leave a half-written stem under the real name


def main():
    device = "cuda" if torch.cuda.is_available() else "cpu"
    if device == "cpu":
        torch.set_num_threads(max(1, (torch.get_num_threads() or 2) - 1))
    model = get_model("htdemucs")
    model.eval()
    vi = model.sources.index("vocals")
    say({"ready": True, "device": device, "model": "htdemucs"})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        job = json.loads(line)
        try:
            wav = AudioFile(job["path"]).read(streams=0, samplerate=model.samplerate, channels=model.audio_channels)
            ref = wav.mean(0)
            mu, sd = ref.mean(), ref.std() + 1e-8
            with torch.no_grad():
                try:
                    src = apply_model(model, ((wav - mu) / sd)[None], device=device, split=True, overlap=0.1, progress=False)[0]
                except torch.cuda.OutOfMemoryError:
                    torch.cuda.empty_cache()
                    src = apply_model(model, ((wav - mu) / sd)[None], device="cpu", split=True, overlap=0.1, progress=False)[0]
            voc = (src[vi] * sd + mu).cpu().numpy().T  # (n, 2)
            write_ogg(job["out"], voc, model.samplerate)
            hop = model.samplerate // 10
            mono = voc.mean(1)
            n = len(mono) // hop
            frames = mono[: n * hop].reshape(n, hop)
            rms = np.sqrt((frames**2).mean(1) + 1e-12)
            env = [round(max(-80.0, 20 * math.log10(r)), 1) for r in rms]
            say({"id": job["id"], "ok": True, "env": env})
        except Exception as e:  # one bad file must not kill the worker
            say({"id": job["id"], "ok": False, "error": f"{type(e).__name__}: {e}"})


if __name__ == "__main__":
    main()
