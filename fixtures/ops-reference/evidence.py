"""Retain non-secret source and artifact identities before disposable builds are removed."""

import hashlib
import json
from pathlib import Path


def native_build(distribution):
    receipts = {}
    for name in ['.lenso/host-build.json', 'local-sources.json', '.lenso/native-app.json',
                 '.lenso/root-linked-sources.json']:
        path = distribution / name
        if path.is_file():
            data = path.read_bytes()
            receipts[name] = {'sha256': hashlib.sha256(data).hexdigest(), 'receipt': json.loads(data)}
    if '.lenso/host-build.json' not in receipts or 'local-sources.json' not in receipts:
        raise ValueError('The ordinary Native distribution lacks its source/build receipts')
    return {'selections': json.loads((Path(__file__).parent / 'candidate-inputs.json').read_text()),
            'receipts': receipts}
