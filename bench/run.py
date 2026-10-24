"""Transcribe one wav with one model/variant set; prints JSON lines. Offline, runs in the vtd-phonon image."""
import json, sys, time, wave
import numpy as np, torch
import transformers as T

name, wavpath = sys.argv[1], sys.argv[2]
with wave.open(wavpath) as w:
    assert w.getframerate() == 16000 and w.getnchannels() == 1
    audio = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768

def split_quiet(a, lo=15.0, hi=25.0, win=0.3):
    """Cut at the quietest 0.3 s window between 15 s and 25 s (models with a ~30-40 s window)."""
    if len(a) / 16000 <= 30:
        return [a]
    n = int(win * 16000)
    best = min(range(int(lo * 16000), int(hi * 16000), 800), key=lambda i: float(np.abs(a[i:i + n]).mean()))
    c = best + n // 2
    return [a[:c], a[c:]]

def emit(variant, text, t0):
    print(json.dumps({"model": name, "variant": variant, "seconds": round(time.time() - t0, 1), "text": text}, ensure_ascii=False), flush=True)

path = f"/models/{name}"
torch.set_num_threads(32)
if name.startswith("whisper"):
    proc = T.WhisperProcessor.from_pretrained(path)
    model = T.WhisperForConditionalGeneration.from_pretrained(path, dtype=torch.float32).eval()
    feats = proc(audio, sampling_rate=16000, return_tensors="pt", truncation=False, padding="longest", return_attention_mask=True)
    hint = "Tohle je nahrávka v češtině a angličtině. This is a recording in Czech and English, switching between them."
    for variant, lang, prompt in [("cs", "cs", None), ("en", "en", None), ("auto", None, None),
                                  ("cs+hint", "cs", hint), ("auto+hint", None, hint)]:
        t0 = time.time()
        kw = dict(task="transcribe", num_beams=5, condition_on_prev_tokens=False, return_timestamps=True)
        if lang: kw["language"] = lang
        if prompt: kw["prompt_ids"] = proc.get_prompt_ids(prompt, return_tensors="pt")
        with torch.inference_mode():
            out = model.generate(**feats, **kw)
        emit(variant, proc.batch_decode(out, skip_special_tokens=True)[0].strip(), t0)
elif name.startswith("canary"):
    proc = T.AutoProcessor.from_pretrained(path)
    model = T.CanaryForConditionalGeneration.from_pretrained(path, dtype=torch.float32).eval()
    for lang in ("cs", "en"):
        t0 = time.time(); parts = []
        for chunk in split_quiet(audio):
            inp = proc.apply_transcription_request(chunk, source_language=lang)
            with torch.inference_mode():
                out = model.generate(**inp, num_beams=5, max_new_tokens=400)
            parts.append(proc.batch_decode(out, skip_special_tokens=True)[0].strip())
        emit(lang, " ".join(parts), t0)
elif name.startswith("qwen"):
    proc = T.AutoProcessor.from_pretrained(path)
    model = T.Qwen3ASRForConditionalGeneration.from_pretrained(path, dtype=torch.float32).eval()
    for variant, lang in [("auto", None), ("cs", "Czech"), ("en", "English")]:
        t0 = time.time(); parts = []
        for chunk in split_quiet(audio):
            text = ("<|im_start|>system\n<|im_end|>\n<|im_start|>user\n<|audio_start|><|audio_pad|><|audio_end|><|im_end|>\n"
                    "<|im_start|>assistant\n" + (f"language {lang}<asr_text>" if lang else ""))
            inp = proc(text=text, audio=chunk, return_tensors="pt")
            with torch.inference_mode():
                out = model.generate(**inp, num_beams=1, max_new_tokens=512)
            raw = proc.batch_decode(out[:, inp["input_ids"].shape[1]:], skip_special_tokens=False)[0]
            raw = raw.replace("<|im_end|>", "").replace("<|endoftext|>", "")
            parts.append(raw.split("<asr_text>")[-1].strip())
        emit(variant, " ".join(parts), t0)
