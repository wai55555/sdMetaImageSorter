#!/usr/bin/env python3
"""Phase 5 の等価性・性能計測用コーパスを生成する（§10.4）。

構成（既定 2,000 件）:
- 70%: メタデータ無し（多数派。改修でコストを増やしてはいけない層）
- 20%: 汎用レコードのみ（Software／Title／XMP CreatorTool だけ。Stealth ゲートが
  「閉じない」側に変わったため、改修後は Stealth 検査が発動する層）
- 5%: Stealth PNG
- 5%: 正例（A1111／NovelAI／ComfyUI／SwarmUI／InvokeAI）

使い方:
    python doc/phase5/generate_phase5_corpus.py --dir target/phase5-corpus --count 2000
    python doc/phase5/generate_phase5_corpus.py --dir target/phase5-generic --count 400 --layer generic
"""
import argparse
import json
import struct
import zlib
from pathlib import Path

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def png_chunk(kind: bytes, data: bytes) -> bytes:
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def simple_png(metadata: bytes = b"", size: int = 1) -> bytes:
    # size x size の RGBA（単色。filter None で各行を zlib 圧縮）
    if size <= 1:
        idat = bytes([120, 156, 99, 96, 0, 2, 0, 0, 5, 0, 1])
    else:
        row = bytes([0]) + bytes([0x20, 0x40, 0x60, 0xFF]) * size
        idat = zlib.compress(row * size, 6)
    return (
        PNG_SIGNATURE
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + metadata
        + png_chunk(b"IDAT", idat)
        + png_chunk(b"IEND", b"")
    )


def text_chunk(keyword: str, text: str) -> bytes:
    return png_chunk(b"tEXt", keyword.encode("latin-1") + b"\0" + text.encode("utf-8"))


def paeth(left: int, above: int, upper_left: int) -> int:
    p = left + above - upper_left
    pa = abs(p - left)
    pb = abs(p - above)
    pc = abs(p - upper_left)
    if pa <= pb and pa <= pc:
        return left
    if pb <= pc:
        return above
    return upper_left


def stealth_png(payload: bytes) -> bytes:
    """fast_meta の stealth_pnginfo / Alpha 8bit 形式（Phase 0 の生成器と同じ）。"""
    signature = b"stealth_pnginfo"
    stream = signature + struct.pack(">I", len(payload) * 8) + payload
    stream_pixel_count = len(stream) * 8
    height = min(stream_pixel_count, 23)
    width = (stream_pixel_count + height - 1) // height
    row_bytes = width * 4
    row_stride = row_bytes + 1
    raw = bytearray(height * row_stride)
    for bit_index in range(len(stream) * 8):
        bit = (stream[bit_index // 8] >> (7 - bit_index % 8)) & 1
        x = bit_index // height
        y = bit_index % height
        raw[y * row_stride + 1 + x * 4 + 3] |= bit

    filtered = bytearray(len(raw))
    for y in range(height):
        row_start = y * row_stride
        filtered[row_start] = y % 5
        for x in range(row_bytes):
            value = raw[row_start + 1 + x]
            left = raw[row_start + 1 + x - 4] if x >= 4 else 0
            above = raw[row_start - row_stride + 1 + x] if y else 0
            upper_left = raw[row_start - row_stride + 1 + x - 4] if y and x >= 4 else 0
            filtered[row_start + 1 + x] = (value - predictor_byte(y % 5, left, above, upper_left)) & 0xFF
    return (
        PNG_SIGNATURE
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(bytes(filtered)))
        + png_chunk(b"IEND", b"")
    )


def predictor_byte(filter_type: int, left: int, above: int, upper_left: int) -> int:
    if filter_type == 0:
        return 0
    if filter_type == 1:
        return left
    if filter_type == 2:
        return above
    if filter_type == 3:
        return (left + above) // 2
    return paeth(left, above, upper_left)


def layers(size: int = 1) -> dict[str, list[bytes]]:
    a1111 = simple_png(
        text_chunk(
            "parameters",
            "a castle\nNegative prompt: low quality\nSteps: 20, Sampler: Euler a, Seed: 1",
        ),
        size=size,
    )
    novelai = simple_png(
        text_chunk("Description", "masterpiece, a cat")
        + text_chunk("Comment", json.dumps({"uc": "blur", "steps": 28, "sampler": "k_euler", "scale": 11}))
        + text_chunk("Software", "NovelAI"),
        size=size,
    )
    comfyui = simple_png(text_chunk("workflow", '{"nodes":[],"links":[]}'), size=size)
    swarmui = simple_png(
        text_chunk("sui_image_params", json.dumps({"prompt": "a castle", "model": "sd_xl_base_1.0"})),
        size=size,
    )
    invokeai = simple_png(
        text_chunk("invokeai_metadata", json.dumps({"prompt": "a castle", "seed": 1})), size=size
    )
    stealth = stealth_png(b'{"Description":"a cat","Source":"NovelAI Diffusion 4E7A","sampler":"k_euler_ancestral","seed":1,"steps":28,"scale":11}')
    return {
        # 多数派: メタデータ無し
        "none": [simple_png(size=size)],
        # 汎用レコードのみ（Stealth ゲートが閉じない層）
        "generic": [
            simple_png(text_chunk("Software", "Celsys Studio Tool"), size=size),
            simple_png(text_chunk("Title", "a title"), size=size),
        ],
        "stealth": [stealth],
        "positive": [a1111, novelai, comfyui, swarmui, invokeai],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", type=Path, required=True)
    parser.add_argument("--count", type=int, default=2000)
    parser.add_argument("--layer", choices=["mixed", "none", "generic", "stealth"], default="mixed")
    parser.add_argument("--pixels", type=int, default=1, help="1 辺の画素数（性能計測用）")
    args = parser.parse_args()

    if args.dir.exists():
        for child in args.dir.rglob("*"):
            if child.is_file():
                child.unlink()
    args.dir.mkdir(parents=True, exist_ok=True)

    groups = layers(size=args.pixels)
    if args.layer == "mixed":
        weights = [("none", 70), ("generic", 20), ("stealth", 5), ("positive", 5)]
    else:
        weights = [(args.layer, 100)]
    total_weight = sum(weight for _, weight in weights)
    for index in range(args.count):
        # 決定的だが偏らない並び（重みどおりの比率にする）
        pick = (index * 37 + 11) % total_weight
        running = 0
        chosen = weights[0][0]
        for name, weight in weights:
            running += weight
            if pick < running:
                chosen = name
                break
        pool = groups[chosen]
        data = pool[index % len(pool)]
        (args.dir / f"{index:06d}_{chosen}.png").write_bytes(data)
    print(f"wrote {args.count} files to {args.dir}")


if __name__ == "__main__":
    main()
