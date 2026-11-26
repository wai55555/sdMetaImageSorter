#!/usr/bin/env python3
"""Phase 0 の合成コーパスを生成する。

外部依存を使わず、PNG / JPEG / WebP の最小構造にメタデータだけを載せる。
`corpus` は等価性確認用（リポジトリにコミットする）、`--perf-dir` は
性能計測用の使い捨てコーパス（既定では target/ 配下）を生成する。
"""

import argparse
import json
import shutil
import struct
import zlib
from pathlib import Path

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def png_chunk(kind: bytes, data: bytes) -> bytes:
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def ihdr(width: int, height: int, color_type: int = 6, bit_depth: int = 8) -> bytes:
    return png_chunk(
        b"IHDR",
        struct.pack(">IIBBBBB", width, height, bit_depth, color_type, 0, 0, 0),
    )


def text_chunk(keyword: str, text: str) -> bytes:
    return png_chunk(b"tEXt", keyword.encode("latin-1") + b"\0" + text.encode("utf-8"))


def simple_png(metadata: bytes = b"") -> bytes:
    raw = b"\0\0\0\0\0"  # 1x1 RGBA, filter None
    return (
        PNG_SIGNATURE
        + ihdr(1, 1)
        + metadata
        + png_chunk(b"IDAT", zlib.compress(raw))
        + png_chunk(b"IEND", b"")
    )


def paeth(left: int, above: int, upper_left: int) -> int:
    predictor = left + above - upper_left
    distances = (abs(predictor - left), abs(predictor - above), abs(predictor - upper_left))
    if distances[0] <= distances[1] and distances[0] <= distances[2]:
        return left
    if distances[1] <= distances[2]:
        return above
    return upper_left


def stealth_png(payload: bytes) -> bytes:
    """fast_meta の stealth_pnginfo / Alpha 8bit 形式を生成する。"""
    signature = b"stealth_pnginfo"
    stream = signature + struct.pack(">I", len(payload) * 8) + payload
    stream_pixel_count = (len(stream) * 8 + 0) // 1
    height = min(stream_pixel_count, 23)
    width = (stream_pixel_count + height - 1) // height
    row_bytes = width * 4
    row_stride = row_bytes + 1
    raw = bytearray(height * row_stride)
    for bit_index in range(len(stream) * 8):
        bit = (stream[bit_index // 8] >> (7 - bit_index % 8)) & 1
        pixel_index = bit_index
        x = pixel_index // height
        y = pixel_index % height
        raw[y * row_stride + 1 + x * 4 + 3] |= bit

    filtered = bytearray(len(raw))
    for y in range(height):
        row_start = y * row_stride
        filter_type = y % 5
        filtered[row_start] = filter_type
        for x in range(row_bytes):
            value = raw[row_start + 1 + x]
            left = raw[row_start + 1 + x - 4] if x >= 4 else 0
            above = raw[row_start - row_stride + 1 + x] if y else 0
            upper_left = raw[row_start - row_stride + 1 + x - 4] if y and x >= 4 else 0
            predictor = {
                0: 0,
                1: left,
                2: above,
                3: (left + above) // 2,
                4: paeth(left, above, upper_left),
            }[filter_type]
            filtered[row_start + 1 + x] = (value - predictor) & 0xFF

    return (
        PNG_SIGNATURE
        + ihdr(width, height)
        + png_chunk(b"IDAT", zlib.compress(bytes(filtered)))
        + png_chunk(b"IEND", b"")
    )


def jpeg_segment(marker: int, data: bytes) -> bytes:
    return b"\xff" + bytes([marker]) + struct.pack(">H", len(data) + 2) + data


def exif_user_comment(text: str) -> bytes:
    """WebP の EXIF チャンク向けに TIFF ヘッダから始まる形で返す。"""
    payload = b"ASCII\0\0\0" + text.encode("utf-8")
    return (
        b"II*\0"
        + struct.pack("<I", 8)
        + struct.pack("<H", 1)
        + bytes([0x86, 0x92])
        + struct.pack("<H", 7)
        + struct.pack("<I", len(payload))
        + struct.pack("<I", 26)
        + struct.pack("<I", 0)
        + payload
    )


def minimal_jpeg(leading_segments: list[bytes]) -> bytes:
    jpeg = bytearray(b"\xff\xd8")
    for segment in leading_segments:
        jpeg += segment
    jpeg += jpeg_segment(0xDB, b"\0" + bytes([1] * 64))
    jpeg += jpeg_segment(0xC0, bytes([8, 0, 8, 0, 8, 1, 1, 0x11, 0]))
    dht = bytearray()
    for table_class in (0, 0x10):
        dht += bytes([table_class, 1])
        dht += bytes([0] * 15)
        dht += b"\0"
    jpeg += jpeg_segment(0xC4, bytes(dht))
    jpeg += jpeg_segment(0xDA, bytes([1, 1, 0, 0, 63, 0]))
    jpeg += b"\x3f\xff\xd9"
    return bytes(jpeg)


def jpeg_with_comment(text: str) -> bytes:
    return minimal_jpeg([jpeg_segment(0xE1, b"Exif\0\0" + exif_user_comment(text))])


def xmp_packet(description: str, creator_tool: str = "Midjourney") -> bytes:
    return (
        '<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
        'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description '
        'xmlns:dc="http://purl.org/dc/elements/1.1/" '
        'xmlns:xmp="http://ns.adobe.com/xap/1.0/">'
        f"<dc:description>{description}</dc:description>"
        f"<xmp:CreatorTool>{creator_tool}</xmp:CreatorTool>"
        "</rdf:Description></rdf:RDF></x:xmpmeta>"
    ).encode("utf-8")


def jpeg_with_xmp(description: str) -> bytes:
    payload = b"http://ns.adobe.com/xap/1.0/\0" + xmp_packet(description)
    return minimal_jpeg([jpeg_segment(0xE1, payload)])


def webp_chunk(kind: bytes, data: bytes) -> bytes:
    return kind + struct.pack("<I", len(data)) + data + (b"\0" if len(data) % 2 else b"")


def webp_with_exif(exif: bytes) -> bytes:
    riff_body = (
        b"WEBP"
        + webp_chunk(b"VP8X", bytes([0x08, 0, 0, 0, 1, 0, 0, 1, 0, 0]))
        + webp_chunk(
            b"VP8L",
            bytes(
                [
                    0x2F,
                    0x01,
                    0x40,
                    0x00,
                    0x00,
                    0x07,
                    0x10,
                    0xF5,
                    0x8F,
                    0xFE,
                    0x07,
                    0x22,
                    0xA2,
                    0xFF,
                    0x01,
                ]
            ),
        )
        + webp_chunk(b"EXIF", exif)
    )
    return b"RIFF" + struct.pack("<I", len(riff_body)) + riff_body


def trailing_png() -> bytes:
    image = simple_png(text_chunk("Title", "plain"))
    # 旧 sorter の本文部分文字列ルール（Software）が IEND 後方でも効く形にする。
    return image + b'{"Software":"Midjourney","parameters":"trailing"}\n'


def corpus_files() -> dict[str, bytes]:
    novelai_pack = json.dumps(
        {
            "Description": "masterpiece, best quality, a cat",
            "Comment": json.dumps({"seed": 1, "steps": 28, "sampler": "k_euler"}),
            "Software": "NovelAI",
            "Source": "NovelAI diffusion",
        },
        ensure_ascii=False,
    )
    return {
        "comfyui.png": simple_png(text_chunk("workflow", '{"nodes":[],"links":[]}')),
        "a1111.png": simple_png(
            text_chunk(
                "parameters",
                "a castle\nNegative prompt: low quality\nSteps: 20, Sampler: Euler a, Seed: 1",
            )
        ),
        "a1111.jpg": jpeg_with_comment(
            "parameters=a castle\nNegative prompt: low quality\nSteps: 20, Seed: 1"
        ),
        "novelai.png": simple_png(
            text_chunk("Description", "masterpiece, best quality, a cat")
            + text_chunk("Comment", json.dumps({"seed": 1, "steps": 28}))
            + text_chunk("Software", "NovelAI")
        ),
        "novelai.webp": webp_with_exif(exif_user_comment(novelai_pack)),
        "swarmui.png": simple_png(
            text_chunk(
                "sui_image_params",
                json.dumps({"prompt": "a castle", "model": "sd_xl_base_1.0"}),
            )
        ),
        "invokeai.png": simple_png(
            text_chunk("invokeai_metadata", json.dumps({"prompt": "a castle", "seed": 1}))
        ),
        "midjourney.png": simple_png(
            text_chunk("Description", "a cat --ar 1:1")
            + text_chunk("Software", "Midjourney")
        ),
        "stealth.png": stealth_png(b"ComfyUI workflow"),
        "no_metadata.png": simple_png(),
        "no_metadata.jpg": minimal_jpeg([]),
        "photo_xmp.jpg": jpeg_with_xmp("a photograph --ar 1:1"),
        "trailing.png": trailing_png(),
    }


def write_corpus(destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in corpus_files().items():
        (destination / name).write_bytes(data)


def write_perf_corpus(destination: Path, count: int) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    sources = corpus_files()
    names = [name for name in sources if name != "trailing.png"]
    for index in range(count):
        name = names[index % len(names)]
        target = destination / f"{index:06d}_{name}"
        target.write_bytes(sources[name])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--dir",
        type=Path,
        default=Path(__file__).resolve().parent / "corpus",
        help="等価性コーパスの出力先",
    )
    parser.add_argument(
        "--perf-dir",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "target" / "phase0-perf",
        help="性能計測用コーパスの出力先（--perf-count 指定時のみ生成）",
    )
    parser.add_argument("--perf-count", type=int, default=0)
    args = parser.parse_args()

    write_corpus(args.dir)
    print(f"corpus: {args.dir}")
    if args.perf_count:
        write_perf_corpus(args.perf_dir, args.perf_count)
        print(f"perf corpus: {args.perf_dir} ({args.perf_count} files)")


if __name__ == "__main__":
    main()
