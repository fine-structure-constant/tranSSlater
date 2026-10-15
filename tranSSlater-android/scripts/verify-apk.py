"""Inspect the produced APK without a connected device."""
from pathlib import Path
import hashlib
import json
import struct
import zipfile

root = Path(__file__).resolve().parents[1]
apk = root / 'artifacts/tranSSlater-arm64.apk'
with zipfile.ZipFile(apk) as archive:
    assert archive.testzip() is None, 'APK ZIP integrity failed'
    libraries = [item for item in archive.infolist() if item.filename.endswith('.so')]
    assert libraries and all('/arm64-v8a/' in item.filename for item in libraries), 'Unexpected APK ABI'
    results = []
    for entry in libraries:
        binary = archive.read(entry)
        assert binary[:5] == b'\x7fELF\x02', 'Not a 64-bit ELF'
        assert struct.unpack_from('<H', binary, 18)[0] == 183, 'Not an AArch64 ELF'
        phoff = struct.unpack_from('<Q', binary, 32)[0]
        phsize, phcount = struct.unpack_from('<HH', binary, 54)
        loads = [struct.unpack_from('<IIQQQQQQ', binary, phoff + i * phsize) for i in range(phcount)]
        alignments = [header[7] for header in loads if header[0] == 1]
        assert alignments and min(alignments) >= 16384, 'ELF lacks 16 KiB page alignment'
        with apk.open('rb') as stream:
            stream.seek(entry.header_offset + 26)
            name_len, extra_len = struct.unpack('<HH', stream.read(4))
        data_offset = entry.header_offset + 30 + name_len + extra_len
        if entry.compress_type == zipfile.ZIP_STORED:
            assert data_offset % 16384 == 0, 'Stored native library is not ZIP-aligned to 16 KiB'
        results.append({'path': entry.filename, 'bytes': len(binary), 'loadAlignments': alignments, 'zipDataOffset': data_offset})

report = {'apk': apk.name, 'bytes': apk.stat().st_size, 'sha256': hashlib.sha256(apk.read_bytes()).hexdigest(), 'abi': 'arm64-v8a', 'libraries': results, 'zipIntegrity': 'passed', 'nativePageAlignment': 'passed', 'deviceLaunch': 'not run: no device or emulator connected'}
(root / 'artifacts/verification.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(json.dumps(report, ensure_ascii=False, indent=2))
