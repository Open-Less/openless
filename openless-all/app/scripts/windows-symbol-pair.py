#!/usr/bin/env python3
"""Validate an MSVC PE/PDB pair without loading code or trusting embedded paths."""
import argparse
import hashlib
import json
import mmap
from pathlib import Path
import struct
import uuid

MSF7 = b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0"


def u32(data, offset):
    return struct.unpack_from("<I", data, offset)[0]


def identity(guid, age):
    return {"guid": str(uuid.UUID(bytes_le=bytes(guid))).upper(), "age": age}


def pe_identity(data):
    if data[:2] != b"MZ":
        raise ValueError("not a PE file")
    pe = u32(data, 0x3C)
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("missing PE signature")
    sections, optional_size = struct.unpack_from("<H12xH", data, pe + 6)
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directory = {0x10B: 96, 0x20B: 112}.get(magic)
    if directory is None:
        raise ValueError("unknown PE optional header")
    debug_rva, debug_size = struct.unpack_from("<II", data, optional + directory + 6 * 8)
    table = optional + optional_size
    for index in range(sections):
        section = table + index * 40
        virtual_size, rva, raw_size, raw = struct.unpack_from("<IIII", data, section + 8)
        if rva <= debug_rva < rva + max(virtual_size, raw_size):
            debug = raw + debug_rva - rva
            for offset in range(debug, debug + debug_size, 28):
                if u32(data, offset + 12) == 2:
                    size, _, pointer = struct.unpack_from("<III", data, offset + 16)
                    if size >= 24 and data[pointer:pointer + 4] == b"RSDS":
                        return identity(data[pointer + 4:pointer + 20], u32(data, pointer + 20))
    raise ValueError("missing RSDS debug identity")


def pdb_identity(data):
    if data[:32] != MSF7:
        raise ValueError("not an MSF7 PDB")
    block_size, _, blocks, directory_size, _, block_map = struct.unpack_from("<6I", data, 32)
    if block_size not in (512, 1024, 2048, 4096) or blocks * block_size > len(data):
        raise ValueError("invalid PDB block layout")
    count = (directory_size + block_size - 1) // block_size
    if count * 4 > block_size:
        raise ValueError("unsupported multi-block PDB directory map")

    def block(index):
        if index >= blocks:
            raise ValueError("PDB block outside file")
        return data[index * block_size:(index + 1) * block_size]

    mapping = block(block_map)
    directory = b"".join(block(u32(mapping, i * 4)) for i in range(count))[:directory_size]
    streams = u32(directory, 0)
    if streams < 2 or 4 + streams * 4 > len(directory):
        raise ValueError("missing PDB info stream")
    sizes = struct.unpack_from(f"<{streams}I", directory, 4)
    cursor = 4 + streams * 4
    for index, size in enumerate(sizes):
        count = 0 if size == 0xFFFFFFFF else (size + block_size - 1) // block_size
        if index == 1:
            if size == 0xFFFFFFFF or size < 28:
                raise ValueError("invalid PDB info stream")
            info = b"".join(block(u32(directory, cursor + i * 4)) for i in range(count))[:size]
            return identity(info[12:28], u32(info, 8))
        cursor += count * 4
    raise ValueError("missing PDB identity")


def file_identity(path, reader):
    with path.open("rb") as file, mmap.mmap(file.fileno(), 0, access=mmap.ACCESS_READ) as data:
        return reader(data)


def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def validate_pair(exe, pdb):
    pe = file_identity(exe, pe_identity)
    symbols = file_identity(pdb, pdb_identity)
    if pe != symbols:
        raise ValueError("EXE and PDB GUID/Age do not match")
    return {"pe_codeview": pe, "pdb_info": symbols,
            "exe_sha256": sha256(exe), "pdb_sha256": sha256(pdb)}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("exe", type=Path)
    parser.add_argument("pdb", type=Path)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--run-url", required=True)
    args = parser.parse_args()
    result = validate_pair(args.exe, args.pdb)
    result.update(source_revision=args.revision, run_url=args.run_url)
    args.manifest.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print("EXE/PDB GUID/Age match; checksums and build identity recorded")
