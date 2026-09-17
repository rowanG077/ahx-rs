import re
import struct
import sys
from pathlib import Path

# Usage: python3 scripts/generate_mpg123.py /path/to/mpg123-1.33.7
src = Path(sys.argv[1]) / "src/libmpg123"
out = Path(__file__).resolve().parents[1] / "src"
notice = "// Derived from mpg123 1.33.7; LGPL-2.1-or-later. See COPYING and NOTICE.\n"
s = (
    (src / "l12tabs.h")
    .read_text()
    .split("real layer12_table[27][64] =")[1]
    .split("};")[0]
)
vals = re.findall(r"[-+]?\d+\.\d+e[-+]\d+f", s)
assert len(vals) == 27 * 64
f = (
    notice
    + "// Preserve the upstream f32 constants without host libm table generation.\npub const MULS: [[f32;64];27] = [\n"
)
for i in range(27):
    f += (
        "["
        + ",".join(
            "f32::from_bits(0x%08x)"
            % struct.unpack("<I", struct.pack("<f", float(v[:-1])))[0]
            for v in vals[i * 64 : i * 64 + 64]
        )
        + "],\n"
    )
f += "];\n"
s = (src / "tabinit.c").read_text().split("intwinbase[] = {")[1].split("};")[0]
vals = re.findall(r"-?\d+", s)
assert len(vals) == 257
f += "pub const WINDOW: [i32;257] = [" + ",".join(vals) + "];\n"
(out / "tables.rs").write_text(f)
# Translate the exact NEON64 DCT operation ordering into portable scalar Rust.
s = (src / "dct64_neon64_float.S").read_text()
costs = re.findall(r"\.word (\d+)", s)
assert len(costs) == 32
regs = {}
lines = []
seq = 0
cp = 0
ptr = [0, 0]


def emit(d, expressions):
    global seq
    n = f"t{seq}"
    seq += 1
    lines.append("let " + n + ": [f32;4] = [" + ", ".join(expressions) + "];")
    regs[d] = [f"{n}[{i}]" for i in range(4)]


def get(n):
    return regs[int(n)]


for line in s.split("ASM_NAME(INT123_dct64_real_neon64):")[1].splitlines():
    line = line.split("/*")[0].strip()
    if not line:
        continue
    op = line.split()[0]
    nums = [int(x) for x in re.findall(r"\bv(\d+)", line)]
    if op == "ld1":
        if "[x2]" in line:
            base = 0
        elif "[x3]" in line:
            base = 16
        else:
            base = cp
            cp += len(nums) * 4
        for j, d in enumerate(nums):
            regs[d] = [
                f"samples[{base + j * 4 + i}]"
                if "[x4]" not in line
                else f"f32::from_bits({costs[base + j * 4 + i]})"
                for i in range(4)
            ]
    elif op == "rev64":
        a = get(nums[1])
        regs[nums[0]] = [a[1], a[0], a[3], a[2]]
    elif op == "ext":
        a = get(nums[1]) + get(nums[2])
        regs[nums[0]] = a[2:6]
    elif op in ("fadd", "fsub", "fmul"):
        d, a, b = nums
        operator = {"fadd": "+", "fsub": "-", "fmul": "*"}[op]
        emit(d, [f"{x} {operator} {y}" for x, y in zip(get(a), get(b))])
    elif op in ("zip1", "zip2"):
        d, a, b = nums
        a = get(a)
        b = get(b)
        if ".2d" in line:
            indices = [0, 1] if op == "zip1" else [2, 3]
            regs[d] = [a[i] for i in indices] + [b[i] for i in indices]
        else:
            indices = [0, 1] if op == "zip1" else [2, 3]
            regs[d] = [v for i in indices for v in (a[i], b[i])]
    elif op in ("uzp1", "uzp2"):
        d, a, b = nums
        regs[d] = (get(a) + get(b))[0 if op == "uzp1" else 1 :: 2]
    elif op.startswith("AARCH64_DUP_"):
        d, a = nums
        k = int(re.search(r",\s*(\d+)\)", line)[1])
        regs[d] = get(a)[k * 2 : k * 2 + 2] * 2 if "2D" in op else [get(a)[k]] * 4
    elif op == "ins":
        d, a = nums
        i, j = map(int, re.findall(r"\[(\d+)\]", line))
        r = get(d).copy()
        v = get(a)
        if ".d[" in line:
            r[i * 2 : i * 2 + 2] = v[j * 2 : j * 2 + 2]
        else:
            r[i] = v[j]
        regs[d] = r
    elif op == "eor":
        regs[nums[0]] = ["0.0"] * 4
    elif op == "st1":
        d = nums[0]
        lane = int(re.search(r"\}\[(\d+)\]", line)[1])
        dest = int(re.search(r"\[x([01])\]", line)[1])
        lines.append(f"out{dest}[{ptr[dest]}] = {get(d)[lane]};")
        ptr[dest] += 16
    elif op in ("add", "adrp", "mov", "ret", "NONEXEC_STACK"):
        pass
    else:
        raise RuntimeError(line)
(out / "dct.rs").write_text(
    notice
    + "// Scalar translation of Taihei Monma's dct64_neon64_float.S.\n// Each binding preserves an upstream f32 rounding point.\npub fn dct(samples: &[f32;32], out0: &mut [f32], out1: &mut [f32]) {\n"
    + "\n".join(lines)
    + "\n}\n"
)
