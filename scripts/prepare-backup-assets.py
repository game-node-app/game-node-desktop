#!/usr/bin/env python3
"""Prepare pinned Windows backup assets; never downloads at application runtime."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def verified(data, expected):
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise ValueError(f'SHA-256 mismatch: expected {expected}, got {actual}')
    return data


def prepare(destination):
    contract = json.loads((ROOT / 'packaging/windows/backup-inputs.json').read_text())
    build_inputs_path = destination / 'build-inputs.json'
    if build_inputs_path.is_file():
        try:
            cached_meta = json.loads(build_inputs_path.read_text())
            if cached_meta.get('contract') == contract:
                all_valid = True
                for name, expected_hash in cached_meta.get('assets', {}).items():
                    asset_file = destination / name
                    if not asset_file.is_file() or hashlib.sha256(asset_file.read_bytes()).hexdigest() != expected_hash:
                        all_valid = False
                        break
                if all_valid and cached_meta.get('assets'):
                    return
        except Exception:
            pass
    # Validate every input before publishing any output.
    inputs = {}
    for name, source in contract['inputs'].items():
        existing_path = destination / name
        if existing_path.is_file():
            data = existing_path.read_bytes()
            if hashlib.sha256(data).hexdigest() == source['sha256']:
                inputs[name] = data
                continue
        with urllib.request.urlopen(source['url'], timeout=60) as response:
            inputs[name] = verified(response.read(), source['sha256'])
    with zipfile.ZipFile(io.BytesIO(inputs.pop('helper_archive'))) as archive:
        executables = [n for n in archive.namelist() if Path(n).name == 'ludusavi.exe']
        if len(executables) != 1:
            raise ValueError('release must contain exactly one ludusavi.exe')
        inputs['ludusavi.exe'] = archive.read(executables[0])
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in inputs.items():
        temporary = destination / (name + '.tmp')
        temporary.write_bytes(data)
        temporary.replace(destination / name)
    metadata = {'contract': contract, 'assets': {
        name: hashlib.sha256(data).hexdigest() for name, data in inputs.items()
    }}
    (destination / 'build-inputs.json').write_text(json.dumps(metadata, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    prepare(parser.parse_args().destination)
