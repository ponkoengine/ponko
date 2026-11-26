#!/usr/bin/env python3
"""Check that the built library exports every function ponko.h declares.

Usage: python3 scripts/test-native-symbols.py path/to/library
The retained ABI-2 baseline client (examples/cpp/abi_baseline.c) separately
checks unchanged C layouts.
"""
import ctypes
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parent.parent
library = ctypes.CDLL(str(Path(sys.argv[1]).resolve()))
header = (root / 'include/ponko.h').read_text(encoding='utf-8')
names = sorted(set(re.findall(r'\b(ponko_\w+)\s*\(', header)))
assert names
for name in names:
    getattr(library, name)
macro = re.search(r'^#define PONKO_ABI_VERSION (\d+)$', header, re.M)
assert macro, 'ponko.h defines no PONKO_ABI_VERSION'
version = library.ponko_abi_version
version.restype = ctypes.c_uint32
assert version() == int(macro.group(1)), 'library and header disagree on the ABI version'
create = library.ponko_world_new
create.restype = ctypes.c_void_p
world = create()
assert world
free = library.ponko_world_free
free.argtypes = [ctypes.c_void_p]
free.restype = ctypes.c_uint32
assert free(world) == 0
print(f'{len(names)} dynamic exports verified.')
