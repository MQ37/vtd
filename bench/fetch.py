"""Fetch benchmark models: safetensors + small json/text files only (no pickles, no .py),
every LFS file checked against the sha256 Hugging Face reports. Run in a container with network."""
import hashlib, sys
from pathlib import Path
from huggingface_hub import HfApi, hf_hub_download

REPOS = {
    "whisper-large-v3": ("openai/whisper-large-v3", {"model.safetensors"}),
    "whisper-large-v3-czech": ("mikr/whisper-large-v3-czech-cv13", {"model.safetensors"}),
    "canary-1b-v2": ("nvidia/canary-1b-v2", {"model.safetensors"}),
    "qwen3-asr-1.7b": ("Qwen/Qwen3-ASR-1.7B", None),
}
SMALL_OK = (".json", ".txt", ".model", ".tiktoken", ".jinja")
dest = Path(sys.argv[1])
api = HfApi()
for name, (repo, weights) in REPOS.items():
    info = api.model_info(repo, files_metadata=True)
    print(f"== {repo} @ {info.sha}")
    for s in info.siblings:
        f = s.rfilename
        is_weight = f.endswith(".safetensors") and (weights is None and "fp32" not in f or weights is not None and f in weights)
        if "/" in f or not (is_weight or (f.endswith(SMALL_OK) and (s.size or 0) < 20_000_000)):
            continue
        p = Path(hf_hub_download(repo, f, revision=info.sha, local_dir=dest / name))
        if s.lfs:
            h = hashlib.sha256()
            with open(p, "rb") as fh:
                for b in iter(lambda: fh.read(1 << 24), b""):
                    h.update(b)
            assert h.hexdigest() == s.lfs.sha256, f"sha256 mismatch for {repo}/{f}"
            print(f"  ok  {f}  {p.stat().st_size/1e6:.0f} MB sha256 verified")
        else:
            print(f"  got {f}")
