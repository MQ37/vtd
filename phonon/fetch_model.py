"""Download the pinned Phonon-2 snapshot into the Hugging Face cache and verify it.

Runs inside the vtd-phonon image during `vtd install` (the only step, besides
the image build, that has network access). The revision and every file's
sha256 are pinned here, so a changed or tampered upstream repo fails the
install instead of being trusted. Stdlib + huggingface_hub only.
"""
import hashlib
import os
import sys
from pathlib import Path

from huggingface_hub import snapshot_download

REPO = "FermionResearch/Phonon-2"
REVISION = "ca1bef26bcd8ef4a7e16d0636d8a77bb25e298ee"
SHA256 = {
    "phonon-2.bps.tar.zst": "98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e",
    "config.json": "422379e411f14dde97174554deead106ec1e4816e14dd78a1bf99517c6dcc663",
    "packed_manifest.json": "690c6bc43bcbcae8df61cff0b2cace7299d023718b6f2aa9d67157407eadaa07",
    "NOTICE": "00624a5043e7ce74029317b024132ca5286116d6fbf3191d226374bc8273789f",
    "reference_transformers.py": "dd51dc042bda14b9578ef797ea9b137b73d09d801f8ce421c7f3eb3b6ddd40d1",
    "fermion_container.py": "5cf172e8cf6b313e951c4ef588ca913c7285b0aa3b92de413c3d7ec2b939c0e4",
    "LICENSE-WEIGHTS-CC-BY-4.0.txt": "9ba9550ad48438d0836ddab3da480b3b69ffa0aac7b7878b5a0039e7ab429411",
    "LICENSE-CODE-Apache-2.0.txt": "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
    # fermion refuses an incomplete snapshot, so the README is part of the set. Upstream's
    # SHA256SUMS.txt lists a stale README hash; this is the hash of the README at REVISION.
    "README.md": "e3cb26cce44e4eadbe43aa5a3cbe494270ab831ffb648acada9fcda77e1b14c9",
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    snapshot = Path(snapshot_download(REPO, revision=REVISION, allow_patterns=list(SHA256)))
    bad = []
    for name, want in SHA256.items():
        got = sha256(snapshot / name)
        print(f"{'ok ' if got == want else 'BAD'} {name} {got[:16]}")
        if got != want:
            bad.append(name)
    if bad:
        print(f"vtd: refusing to use {REPO}@{REVISION[:8]}: checksum mismatch for {', '.join(bad)}", file=sys.stderr)
        return 1
    # fermion resolves the model via the `main` ref when the hub is offline.
    refs = Path(os.environ.get("HF_HOME", "~/.cache/huggingface")).expanduser() / "hub" / "models--FermionResearch--Phonon-2" / "refs"
    refs.mkdir(parents=True, exist_ok=True)
    (refs / "main").write_text(REVISION)
    print(f"vtd: {REPO}@{REVISION[:8]} verified")
    return 0


sys.exit(main())
