"""Independent reference decode for a lossless-JPEG (SOF3) Linear-Raw DNG.

Decodes the strip restart-interval by restart-interval with libjpeg (via
imagecodecs), so the result does not depend on the Rust decoder under test.
Single-call decoders ignore DRI and silently repeat the first interval, which
is why the stream is split on RSTn markers here.

Usage: python research/samsung_reference_decode.py <file.dng> <out.npy>
Requires imagecodecs, numpy, and tifffile; for research only, not production.
"""
import re
import struct
import sys

import imagecodecs
import numpy as np
import tifffile


def main(path: str, out_path: str) -> None:
    with tifffile.TiffFile(path) as tf:
        page = tf.pages[0]
        offset, count = page.dataoffsets[0], page.databytecounts[0]
        width, height = page.imagewidth, page.imagelength
    with open(path, "rb") as source:
        data = source.read()
    stream = data[offset:offset + count]

    sos = stream.find(b"\xff\xda")
    if sos < 0:
        raise ValueError("missing JPEG scan header")
    sos_len = struct.unpack(">H", stream[sos + 2:sos + 4])[0]
    header_end = sos + 2 + sos_len
    head = bytearray(stream[:header_end])
    dri = head.find(b"\xff\xdd")
    if dri < 0:
        raise ValueError("missing restart interval")
    restart = struct.unpack(">H", head[dri + 4:dri + 6])[0]
    if restart == 0 or restart % width:
        raise ValueError("restart interval must span complete rows")
    del head[dri:dri + 6]
    rows_per_interval = restart // width

    markers = re.findall(rb"\xff[\xd0-\xd7]", stream[header_end:-2])
    parts = re.split(rb"\xff[\xd0-\xd7]", stream[header_end:-2])
    expected = (height + rows_per_interval - 1) // rows_per_interval
    if len(parts) != expected or any(marker[1] != 0xD0 + k % 8 for k, marker in enumerate(markers)):
        raise ValueError("missing or out-of-order restart marker")
    out = np.zeros((height, width, 3), np.uint16)
    for k, part in enumerate(parts):
        rows = min(rows_per_interval, height - k * rows_per_interval)
        piece = bytearray(head)
        sof = piece.find(b"\xff\xc3")
        if sof < 0:
            raise ValueError("not a lossless JPEG")
        piece[sof + 5:sof + 7] = struct.pack(">H", rows)
        image = imagecodecs.ljpeg_decode(bytes(piece) + part + b"\xff\xd9")
        if image.shape != (rows, width, 3):
            raise ValueError(f"interval {k}: unexpected decoded shape {image.shape}")
        out[k * rows_per_interval:k * rows_per_interval + rows] = image
    np.save(out_path, out)
    print(f"decoded {out.shape} in {len(parts)} intervals, range {out.min()}..{out.max()}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
