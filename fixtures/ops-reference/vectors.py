"""The same HTTP vectors are consumed by the Native and Workers profiles."""

from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def request(base, instance, payload=None):
    return request_path(base, '/state/' + instance, payload)


def request_path(base, path, payload=None):
    if os.environ.get('LENSO_REFERENCE_HTTP_TRANSPORT') == 'node':
        result = subprocess.run(['node', str(Path(__file__).with_name('node_http.mjs'))],
            input=json.dumps({'url': base.rstrip('/')+path, 'payload': payload}),
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
        if result.returncode:
            raise RuntimeError('The remote HTTP result is unknown; no request was replayed: '+result.stderr)
        status, value = json.loads(result.stdout)
        return status, value
    body = None if payload is None else json.dumps(payload).encode()
    req = Request(base.rstrip('/') + path, data=body,
                  headers={'Content-Type': 'application/json',
                           'User-Agent': 'lenso-reference-qualification/1'})
    try:
        response = urlopen(req, timeout=10)
    except HTTPError as error:
        response = error
    with response:
        return response.status, json.load(response)


def verify(base):
    results = []
    primary = {'label': 'primary-state', 'value': 11, 'revision': 0}
    secondary = {'label': 'secondary-state', 'value': 29, 'revision': 0}
    assert request(base, 'primary') == (200, primary)
    assert request(base, 'secondary') == (200, secondary)
    results.append('two_configured_instances')

    write = {'value': 47, 'expected_revision': 0, 'idempotency_key': 'write-1'}
    status, receipt = request(base, 'primary', write)
    assert status == 200 and receipt['value'] == 47 and receipt['revision'] == 1
    assert receipt['label'] == 'primary-state' and receipt['receipt_id']
    assert request(base, 'primary', write) == (200, receipt)
    status, query = request_path(base, '/receipts/primary/write-1')
    assert status == 200 and query == {'found': True, **receipt}
    assert request(base, 'secondary') == (200, secondary)
    results.extend(['named_dependency_isolation', 'idempotent_result_query'])

    assert request(base, 'primary', {**write, 'value': 48})[0] == 409
    assert request(base, 'primary', {**write, 'idempotency_key': 'stale'})[0] == 409
    assert request(base, 'primary', {**write, 'idempotency_key': 'bad key'})[0] == 400
    assert request(base, 'primary', {**write, 'value': 9_007_199_254_740_992})[0] == 400
    assert request(base, 'primary', {**write, 'expected_revision': 9_007_199_254_740_991})[0] == 400
    assert request(base, 'primary')[1]['revision'] == 1
    results.extend(['idempotency_intent_conflict', 'stale_revision', 'invalid_input'])

    contenders = [
        {'value': value, 'expected_revision': 1, 'idempotency_key': f'race-{value}'}
        for value in [71, 83]
    ]
    with ThreadPoolExecutor(max_workers=2) as executor:
        outcomes = list(executor.map(lambda command: request(base, 'primary', command), contenders))
    assert sorted(status for status, _ in outcomes) == [200, 409], outcomes
    final = request(base, 'primary')[1]
    assert final['revision'] == 2 and final['value'] in [71, 83]
    assert request(base, 'secondary') == (200, secondary)
    assert request(base, 'missing')[0] == 404
    results.extend(['concurrent_cas_single_commit', 'unknown_instance_rejected'])
    return results


def verify_static(base):
    """G1 deliberately tests routing and config without claiming event persistence."""
    assert request(base, 'primary') == (200, {'label': 'primary-state', 'value': 11, 'revision': 0})
    assert request(base, 'secondary') == (200, {'label': 'secondary-state', 'value': 29, 'revision': 0})
    assert request(base, 'missing')[0] == 404
    return ['two_configured_instances', 'named_dependency_isolation', 'unknown_instance_rejected']


def verify_persistent(base):
    """Query the original commands after a Host or Worker reconstruction."""
    status, state = request(base, 'primary')
    assert status == 200 and state['revision'] == 2 and state['value'] in [71, 83], state
    assert request(base, 'secondary') == (200, {'label': 'secondary-state', 'value': 29, 'revision': 0})
    status, query = request_path(base, '/receipts/primary/write-1')
    assert status == 200 and query['found'] and query['revision'] == 1 and query['value'] == 47
    write = {'value': 47, 'expected_revision': 0, 'idempotency_key': 'write-1'}
    assert request(base, 'primary', write) == (200, {key: value for key, value in query.items() if key != 'found'})
    assert request(base, 'primary')[1] == state
    return ['state_revision_retained', 'secondary_isolation', 'receipt_query_after_restart',
            'idempotent_write_after_restart']
