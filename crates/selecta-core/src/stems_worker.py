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
import time

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


class DevicePicker:
    """Run each track on whichever device is faster *right now*.

    A laptop GPU can be power- or heat-capped to a fraction of its clocks
    (measured: a GTX 1650 Ti held at 300 MHz ran htdemucs at 0.2x realtime
    while its CPU managed 2x). So speed is measured per track, the CPU is
    tried when the GPU is slow, and the loser is re-checked every few tracks
    because throttling comes and goes.
    """

    RECHECK = 8

    def __init__(self, has_cuda):
        self.speed = {"cuda": None, "cpu": None} if has_cuda else {"cpu": None}
        self.n = 0

    def pick(self):
        self.n += 1
        known = {d: v for d, v in self.speed.items() if v is not None}
        unknown = [d for d, v in self.speed.items() if v is None]
        if not known:
            return "cuda" if "cuda" in self.speed else "cpu"
        best = max(known, key=known.get)
        if unknown and known[best] < 1.5:
            return unknown[0]  # current device is slow: measure the other one
        if len(self.speed) > 1 and self.n % self.RECHECK == 0:
            return min(self.speed, key=lambda d: self.speed[d] or 0)  # re-check the slower one
        return best

    def record(self, device, audio_sec, took):
        v = audio_sec / max(took, 1e-3)
        old = self.speed.get(device)
        self.speed[device] = v if old is None else 0.5 * old + 0.5 * v


def main():
    has_cuda = torch.cuda.is_available()
    torch.set_num_threads(max(1, (os.cpu_count() or 2) - 2))  # leave room for the app and audio
    model = get_model("htdemucs")
    model.eval()
    vi = model.sources.index("vocals")
    picker = DevicePicker(has_cuda)
    say({"ready": True, "device": "cuda" if has_cuda else "cpu", "model": "htdemucs"})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        job = json.loads(line)
        try:
            wav = AudioFile(job["path"]).read(streams=0, samplerate=model.samplerate, channels=model.audio_channels)
            ref = wav.mean(0)
            mu, sd = ref.mean(), ref.std() + 1e-8
            device = picker.pick()
            t0 = time.time()
            with torch.no_grad():
                try:
                    src = apply_model(model, ((wav - mu) / sd)[None], device=device, split=True, overlap=0.1, progress=False)[0]
                except torch.cuda.OutOfMemoryError:
                    torch.cuda.empty_cache()
                    device = "cpu"
                    src = apply_model(model, ((wav - mu) / sd)[None], device=device, split=True, overlap=0.1, progress=False)[0]
            picker.record(device, wav.shape[-1] / model.samplerate, time.time() - t0)
            voc = (src[vi] * sd + mu).cpu().numpy().T  # (n, 2)
            write_ogg(job["out"], voc, model.samplerate)
            hop = model.samplerate // 10
            mono = voc.mean(1)
            n = len(mono) // hop
            frames = mono[: n * hop].reshape(n, hop)
            rms = np.sqrt((frames**2).mean(1) + 1e-12)
            env = [round(max(-80.0, 20 * math.log10(r)), 1) for r in rms]
            say({"id": job["id"], "ok": True, "env": env, "device": device, "speed": round(picker.speed[device], 2)})
        except Exception as e:  # one bad file must not kill the worker
            say({"id": job["id"], "ok": False, "error": f"{type(e).__name__}: {e}"})


if __name__ == "__main__":
    main()
