#!/usr/bin/env python3
"""Phase 5 の走査最適化を v2.0.0 と v2.0.1 で比較する。

同じ合成コーパス（1,000 枚）を、フラット配置と 100 ディレクトリのネスト配置で
それぞれ生成し、ウォームアップ 1 回＋3 回の中央値を記録する。
"""

import importlib.util
import shutil
import statistics
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GENERATOR = ROOT / "doc" / "phase0" / "generate_corpus.py"


def load_generator():
    spec = importlib.util.spec_from_file_location("phase0_generate_corpus", GENERATOR)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build_corpus(destination: Path, dirs: int, count: int) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    samples = load_generator().corpus_files()
    names = [name for name in samples if name != "trailing.png"]
    for index in range(count):
        directory = destination / (f"dir{index % dirs:03d}" if dirs > 1 else "")
        directory.mkdir(parents=True, exist_ok=True)
        name = names[index % len(names)]
        (directory / f"{index:06d}_{name}").write_bytes(samples[name])


def measure(exe: Path, master: Path, work: Path) -> list[float]:
    times = []
    for run in range(4):
        if work.exists():
            shutil.rmtree(work)
        shutil.copytree(master, work)
        started = time.perf_counter()
        result = subprocess.run(
            [str(exe), str(work.relative_to(ROOT))], capture_output=True, text=True
        )
        elapsed = time.perf_counter() - started
        if result.returncode != 0:
            raise SystemExit(f"{exe} failed: {result.stdout}\n{result.stderr}")
        if run == 0:
            print(f"  warmup: {elapsed:.3f}s (discarded)")
        else:
            print(f"  run{run}: {elapsed:.3f}s")
            times.append(elapsed)
    return times


def main() -> None:
    binaries = {
        "v2.0.0": ROOT / "target" / "release" / "sd_meta_sorter-v2.0.0.exe",
        "v2.0.1": ROOT / "target" / "release" / "sd_meta_sorter.exe",
    }
    for layout, dirs in (("flat", 1), ("nested", 100)):
        master = ROOT / "target" / f"phase5-{layout}-master"
        work = ROOT / "target" / f"phase5-{layout}-work"
        build_corpus(master, dirs, 1000)
        print(f"== {layout} ({dirs} dirs, 1000 files) ==")
        for version, exe in binaries.items():
            if not exe.exists():
                raise SystemExit(f"missing binary: {exe}")
            print(f"{version}:")
            times = measure(exe, master, work)
            print(f"  median: {statistics.median(times):.3f}s")


if __name__ == "__main__":
    main()
