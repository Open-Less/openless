"""Synthetic format/mismatch checks; executed in Actions, never load a PE."""
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("symbols", Path(__file__).with_name("windows-symbol-pair.py"))
symbols = importlib.util.module_from_spec(spec)
spec.loader.exec_module(symbols)
GUID = bytes(range(16))


def pe_fixture(age=3):
    data = bytearray(1024)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", data, 0x86, 1)
    struct.pack_into("<H", data, 0x94, 240)
    struct.pack_into("<H", data, 0x98, 0x20B)
    struct.pack_into("<II", data, 0x98 + 112 + 48, 0x1000, 28)
    struct.pack_into("<IIII", data, 0x98 + 240 + 8, 512, 0x1000, 512, 512)
    struct.pack_into("<IIII", data, 512 + 12, 2, 24, 0x1040, 576)
    data[576:580] = b"RSDS"
    data[580:596] = GUID
    struct.pack_into("<I", data, 596, age)
    return data


def pdb_fixture(age=3):
    data = bytearray(512 * 5)
    data[:32] = symbols.MSF7
    struct.pack_into("<6I", data, 32, 512, 1, 5, 16, 0, 2)
    struct.pack_into("<I", data, 1024, 3)
    struct.pack_into("<4I", data, 1536, 2, 0, 28, 4)
    struct.pack_into("<3I", data, 2048, 20000404, 0, age)
    data[2060:2076] = GUID
    return data


class SymbolPairTests(unittest.TestCase):
    def test_guid_and_age_from_both_formats(self):
        self.assertEqual(symbols.pe_identity(pe_fixture()), symbols.pdb_identity(pdb_fixture()))
        self.assertEqual(symbols.pe_identity(pe_fixture())["age"], 3)

    def test_mismatch_rejected_and_matched_pair_hashed(self):
        with tempfile.TemporaryDirectory() as directory:
            exe, pdb = Path(directory) / "app.exe", Path(directory) / "app.pdb"
            exe.write_bytes(pe_fixture())
            pdb.write_bytes(pdb_fixture(age=4))
            with self.assertRaisesRegex(ValueError, "do not match"):
                symbols.validate_pair(exe, pdb)
            changed_guid = pdb_fixture()
            changed_guid[2060] ^= 1
            pdb.write_bytes(changed_guid)
            with self.assertRaisesRegex(ValueError, "do not match"):
                symbols.validate_pair(exe, pdb)
            pdb.write_bytes(pdb_fixture())
            result = symbols.validate_pair(exe, pdb)
            self.assertEqual(len(result["exe_sha256"]), 64)
            self.assertEqual(len(result["pdb_sha256"]), 64)

    def test_truncated_format_rejected(self):
        for size in (1, 16, 32, 64, 100):
            with self.assertRaises((ValueError, struct.error, IndexError)):
                symbols.pe_identity(pe_fixture()[:size])
            with self.assertRaises((ValueError, struct.error, IndexError)):
                symbols.pdb_identity(pdb_fixture()[:size])

    def test_missing_symbols_rejected(self):
        with self.assertRaises(ValueError):
            symbols.pe_identity(bytes(1024))
        with self.assertRaises(ValueError):
            symbols.pdb_identity(bytes(1024))


if __name__ == "__main__":
    unittest.main()
