"""Qwen3-ASR through its official qwen-asr package (older transformers in /opt/qwenlib). Offline."""
import sys, json, time, wave
sys.path.insert(0, "/opt/qwenlib")
import numpy as np, torch
from qwen_asr import Qwen3ASRModel
with wave.open(sys.argv[1]) as w:
    audio = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768
torch.set_num_threads(32)
model = Qwen3ASRModel.from_pretrained("/models/qwen3-asr-1.7b", dtype=torch.float32, device_map="cpu", max_inference_batch_size=1, max_new_tokens=1024)
for variant, lang in [("auto", None), ("cs", "Czech"), ("en", "English")]:
    t0 = time.time()
    r = model.transcribe(audio=(audio, 16000), language=lang)[0]
    print(json.dumps({"model": "qwen3-asr-1.7b", "variant": variant, "seconds": round(time.time() - t0, 1), "detected": r.language, "text": r.text.strip()}, ensure_ascii=False), flush=True)
