"""Explicit operator-run download on the GPU host, never on desktop startup.

Resolve exact HF revisions, use the official repositories (no mirror), and write
a reproducibility receipt with hashes. This does not imply dependency/model
licenses are all identical; see the official MuseTalk README and model cards.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--accept-licenses", action="store_true", required=True)
    args = parser.parse_args()
    if not args.accept_licenses:
        raise SystemExit("Review model/dependency licenses first")
    os.environ.pop("HF_HUB_OFFLINE", None)
    os.environ.pop("TRANSFORMERS_OFFLINE", None)
    os.environ["HF_ENDPOINT"] = "https://huggingface.co"
    from huggingface_hub import HfApi, hf_hub_download
    import gdown
    root = Path(os.environ.get("CONCAT_MUSETALK_ROOT", "/opt/MuseTalk")) / "models"
    root.mkdir(parents=True, exist_ok=True)
    specs = [
        ("TMElyralab/MuseTalk", [("musetalkV15/unet.pth", "musetalkV15/unet.pth"), ("musetalkV15/musetalk.json", "musetalkV15/musetalk.json")]),
        ("stabilityai/sd-vae-ft-mse", [(n, "sd-vae/" + n) for n in ["config.json", "diffusion_pytorch_model.bin"]]),
        ("openai/whisper-tiny", [(n, "whisper/" + n) for n in ["config.json", "pytorch_model.bin", "preprocessor_config.json"]]),
        ("yzd-v/DWPose", [("dw-ll_ucoco_384.pth", "dwpose/dw-ll_ucoco_384.pth")]),
    ]
    import shutil
    import urllib.request
    receipt = {"model": "musetalk-1.5", "upstream_code_revision": "0a89dec45a0192b824e3cf4daf96c239440c5ed8", "repositories": {}, "sha256": {}}
    api = HfApi(endpoint="https://huggingface.co")
    for repo, files in specs:
        revision = api.model_info(repo).sha
        receipt["repositories"][repo] = revision
        for remote, relative in files:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            cached = hf_hub_download(repo, remote, revision=revision, endpoint="https://huggingface.co")
            shutil.copyfile(cached, path)
    parser_root = root / "face-parse-bisent"
    parser_root.mkdir(exist_ok=True)
    if not gdown.download(id="154JgKpzCPW82qINcVieuPH3fZ2e0P812", output=str(parser_root / "79999_iter.pth"), quiet=False):
        raise RuntimeError("Official face-parser download failed")
    urllib.request.urlretrieve("https://download.pytorch.org/models/resnet18-5c106cde.pth", parser_root / "resnet18-5c106cde.pth")
    for path in root.rglob("*"):
        if path.is_file() and path.suffix in {".pth", ".bin", ".json"}:
            digest = hashlib.sha256()
            with path.open("rb") as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
            receipt["sha256"][str(path.relative_to(root))] = digest.hexdigest()
    (root / "concat-model-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print("Model files downloaded. GPU inference has NOT been verified yet.")


if __name__ == "__main__":
    main()
