"""Keep storage unavailability distinct from credential denial in a local graph."""
import argparse
import json
from pathlib import Path
from urllib.parse import urlsplit

from verify_graph import approved_origin, call, credential, read, record


def remote(options, alice, bob):
    approved_origin(options.origin, options.remote_facts)
    baseline_path = options.directory / 'healthy-before.json'
    if options.phase == 'before':
        status, catalog = call(options.origin, '/management/catalog', bob)
        assert status == 200
        entry = next(value for value in catalog['entries'] if value['id'] == 'state.read')
        baseline = read(options.origin, bob, entry)
        assert call(options.origin, '/management/catalog')[0] == 401
        assert call(options.origin, '/management/catalog', alice)[0] == 200
        record(baseline_path, {'entry': entry, 'business_state': baseline,
                              'alice_and_bob_live': True, 'anonymous_status': 401})
    elif options.phase == 'fault':
        assert baseline_path.is_file()
        for token in [None, alice, bob]:
            status, unavailable = call(options.origin, '/management/catalog', token)
            assert status == 503 and unavailable == {'error': 'host_unavailable'}
        record(options.directory / 'fault-observed.json', {
            'status': 503, 'result': 'host-unavailable-before-kernel-ready',
            'authentication_after_ready_fault': 'not_exercised', 'no_store': True})
    else:
        assert options.phase == 'restored'
        baseline = json.loads(baseline_path.read_text())
        assert (options.directory / 'fault-observed.json').is_file()
        assert call(options.origin, '/management/catalog')[0] == 401
        for token in [alice, bob]:
            assert call(options.origin, '/management/catalog', token)[0] == 200
            assert read(options.origin, token, baseline['entry']) == baseline['business_state']
        record(options.directory / 'auth-storage-unavailable.json', {
            'schema': 'lenso.qualification.receipt.v1',
            'layer': 'ordinary-source-worker-graph', 'infrastructure': 'cloudflare-workers',
            'fault': 'isolated-uninitialized-auth-binding',
            'result': '503-host-unavailable-before-kernel-ready',
            'credential_denial': 'healthy-401-without-evidence-before-and-after',
            'authentication_after_ready_fault': 'not_exercised',
            'original_alice_and_bob_live': True, 'owner_setup': 'not_performed',
            'business_state_unchanged': baseline['business_state'], 'no_store': True})
    print(json.dumps({'fault': 'auth-storage-unavailable', 'phase': options.phase,
                      'result': 'passed'}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--origin', required=True)
    parser.add_argument('--control-origin')
    parser.add_argument('--alice-file', type=Path, required=True)
    parser.add_argument('--bob-file', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--remote-facts', type=Path)
    parser.add_argument('--phase', choices=['before', 'fault', 'restored'])
    options = parser.parse_args()
    options.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    alice, bob = [credential(getattr(options, name + '_file')) for name in ['alice', 'bob']]
    if options.remote_facts:
        assert options.phase is not None and options.control_origin is None
        remote(options, alice, bob)
        return
    assert options.phase is None and options.control_origin is not None
    for value in [options.origin, options.control_origin]:
        origin = urlsplit(value)
        assert origin.scheme == 'http' and origin.hostname == '127.0.0.1'
        assert origin.port and not origin.username and not origin.password
        assert not origin.path and not origin.query and not origin.fragment
    assert options.origin != options.control_origin
    status, catalog = call(options.control_origin, '/management/catalog', bob)
    assert status == 200
    entry = next(value for value in catalog['entries'] if value['id'] == 'state.read')
    baseline = read(options.control_origin, bob, entry)
    assert call(options.control_origin, '/management/catalog')[0] == 401
    for token in [None, alice, bob]:
        status, unavailable = call(options.origin, '/management/catalog', token)
        assert status == 503 and unavailable == {'error': 'host_unavailable'}
    assert call(options.control_origin, '/management/catalog', alice)[0] == 200
    assert call(options.control_origin, '/management/catalog', bob)[0] == 200
    assert read(options.control_origin, bob, entry) == baseline
    record(options.directory / 'auth-storage-unavailable.json', {
        'schema': 'lenso.qualification.receipt.v1', 'layer': 'ordinary-source-worker-graph',
        'infrastructure': 'local-workerd', 'fault': 'isolated-uninitialized-auth-binding',
        'result': '503-host-unavailable-before-kernel-ready',
        'credential_denial': 'healthy-control-401-without-evidence',
        'authentication_after_ready_fault': 'not_exercised',
        'no_store': True, 'original_alice_and_bob_live': True,
        'owner_setup': 'not_performed', 'business_state_unchanged': baseline})
    print(json.dumps({'fault': 'auth-storage-unavailable', 'result': 'passed'}))


if __name__ == '__main__':
    main()
