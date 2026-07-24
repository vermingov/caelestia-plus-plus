#!/usr/bin/env python3
"""List render-capable GPUs as JSON for the per-app GPU picker.

One entry per /dev/dri/renderD* node: short marketing name (from lspci),
kernel driver, PCI slot and the render node path. The slot is the stable
key the shell stores per app; the driver decides which offload env vars
the launcher injects (DRI_PRIME for mesa, __NV_PRIME_RENDER_OFFLOAD for
the NVIDIA proprietary driver).
"""

import glob
import json
import os
import re
import subprocess


def short_name(slot: str) -> str:
    try:
        desc = subprocess.check_output(["lspci", "-s", slot], text=True).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return slot
    desc = desc.split(": ", 1)[-1]
    desc = re.sub(r"\s*\(rev [^)]*\)$", "", desc)
    brackets = re.findall(r"\[([^\]]+)\]", desc)
    # Vendor tag like [AMD/ATI] comes first; the model like [Radeon 680M] last
    return brackets[-1] if brackets else desc


def main() -> None:
    gpus = []
    for node in sorted(glob.glob("/sys/class/drm/renderD*")):
        uevent = {}
        try:
            for line in open(f"{node}/device/uevent"):
                key, _, value = line.strip().partition("=")
                uevent[key] = value
        except OSError:
            continue
        slot = uevent.get("PCI_SLOT_NAME", "")
        gpus.append({
            "name": short_name(slot) if slot else os.path.basename(node),
            "driver": uevent.get("DRIVER", ""),
            "slot": slot,
            "node": f"/dev/dri/{os.path.basename(node)}",
        })
    print(json.dumps(gpus))


if __name__ == "__main__":
    main()
