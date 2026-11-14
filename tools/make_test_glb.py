#!/usr/bin/env python3
"""Gera um GLB minimo de teste (caixa com animacoes idle/walk/death) para validar o carregador
de modelos do SpiderDesk sem precisar de um modelo real.

Uso:  python3 tools/make_test_glb.py saida/assets/models/spider.glb
Depois: BEVY_ASSET_ROOT=saida cargo run

Convencoes do modelo: frente = +X, cima = +Y, pes em y = 0. As animacoes sao propositalmente
incompletas (faltam run/climb/hit) para exercitar o fallback.
"""
import json
import struct
import sys

import numpy as np


def box(sx, sy, sz, cy):
    hx, hy, hz = sx / 2, sy / 2, sz / 2
    faces = [  # (normal, 4 cantos)
        ((1, 0, 0), [(hx, -hy, -hz), (hx, hy, -hz), (hx, hy, hz), (hx, -hy, hz)]),
        ((-1, 0, 0), [(-hx, -hy, hz), (-hx, hy, hz), (-hx, hy, -hz), (-hx, -hy, -hz)]),
        ((0, 1, 0), [(-hx, hy, -hz), (-hx, hy, hz), (hx, hy, hz), (hx, hy, -hz)]),
        ((0, -1, 0), [(-hx, -hy, hz), (-hx, -hy, -hz), (hx, -hy, -hz), (hx, -hy, hz)]),
        ((0, 0, 1), [(-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz)]),
        ((0, 0, -1), [(hx, -hy, -hz), (-hx, -hy, -hz), (-hx, hy, -hz), (hx, hy, -hz)]),
    ]
    pos, nor, idx = [], [], []
    for n, corners in faces:
        base = len(pos)
        for c in corners:
            pos.append((c[0], c[1] + cy, c[2]))
            nor.append(n)
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return (np.array(pos, np.float32), np.array(nor, np.float32), np.array(idx, np.uint16))


def quat(axis, deg):
    a = np.radians(deg) / 2
    ax = np.array(axis, np.float32)
    ax = ax / np.linalg.norm(ax)
    return [*(ax * np.sin(a)), np.cos(a)]


def main(out):
    pos, nor, idx = box(2.0, 0.7, 1.1, 0.75)
    blobs = []  # (bytes, target)
    views, accessors = [], []

    def add(data, target=None, **acc):
        raw = data.tobytes()
        pad = (-len(raw)) % 4
        offset = sum(len(b) for b in blobs)
        blobs.append(raw + b"\0" * pad)
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(raw), **({"target": target} if target else {})})
        accessors.append({"bufferView": len(views) - 1, "componentType": acc["ct"], "count": acc["count"], "type": acc["type"],
                          **({"min": acc["min"], "max": acc["max"]} if "min" in acc else {})})
        return len(accessors) - 1

    a_pos = add(pos, 34962, ct=5126, count=len(pos), type="VEC3", min=pos.min(0).tolist(), max=pos.max(0).tolist())
    a_nor = add(nor, 34962, ct=5126, count=len(nor), type="VEC3")
    a_idx = add(idx, 34963, ct=5123, count=len(idx), type="SCALAR")

    def anim(name, path, times, values, typ):
        t = np.array(times, np.float32)
        v = np.array(values, np.float32)
        ai = add(t, ct=5126, count=len(t), type="SCALAR", min=[float(t.min())], max=[float(t.max())])
        ao = add(v, ct=5126, count=len(v), type=typ)
        return {"name": name, "samplers": [{"input": ai, "output": ao, "interpolation": "LINEAR"}],
                "channels": [{"sampler": 0, "target": {"node": 1, "path": path}}]}

    animations = [
        anim("Armature|idle", "scale", [0, 1, 2], [[1, 1, 1], [1, 1.06, 1], [1, 1, 1]], "VEC3"),
        anim("Armature|walk", "rotation", [0, 0.25, 0.5, 0.75, 1.0],
             [quat((1, 0, 0), 0), quat((1, 0, 0), 8), quat((1, 0, 0), 0), quat((1, 0, 0), -8), quat((1, 0, 0), 0)], "VEC4"),
        anim("Armature|death", "rotation", [0, 1.0], [quat((1, 0, 0), 0), quat((1, 0, 0), 180)], "VEC4"),
    ]

    gltf = {
        "asset": {"version": "2.0", "generator": "spiderdesk-tools"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "Armature", "children": [1]}, {"name": "Body", "mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": a_pos, "NORMAL": a_nor}, "indices": a_idx, "material": 0}]}],
        "materials": [{"name": "teste", "pbrMetallicRoughness": {"baseColorFactor": [0.9, 0.35, 0.1, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.6}}],
        "animations": animations,
        "buffers": [{"byteLength": sum(len(b) for b in blobs)}],
        "bufferViews": views,
        "accessors": accessors,
    }
    js = json.dumps(gltf, separators=(",", ":")).encode()
    js += b" " * ((-len(js)) % 4)
    bin_ = b"".join(blobs)
    total = 12 + 8 + len(js) + 8 + len(bin_)
    import os
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(bin_), b"BIN\0") + bin_)
    print(f"GLB escrito: {out} ({total} bytes)")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
