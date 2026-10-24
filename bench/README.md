# Heavy-model benchmark (reference only)

How the `--heavy` model was chosen: whisper large-v3 (full), a Czech fine-tune, Qwen3-ASR-1.7B and
Canary-1B-v2 against one hand-checked Czech/English recording, scored with `wer.py`. Everything ran
offline in podman on top of the `vtd-phonon` image (`run.py`, `qwen_run.py`); `fetch.py` downloads
pinned, sha256-verified safetensors only. Not used by `vtd` itself.
