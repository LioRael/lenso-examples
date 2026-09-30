"""Exercise an ordinary, already prepared local Worker graph without Owner setup."""
import argparse
import json
import os
from pathlib import Path
import stat
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen


def approved_origin(value, remote_facts=None):
    origin = urlsplit(value)
    assert not origin.username and not origin.password
    assert not origin.path and not origin.query and not origin.fragment
    if remote_facts is None:
        assert origin.scheme == 'http' and origin.hostname == '127.0.0.1'
        assert origin.port is not None and 1 <= origin.port <= 65535
    else:
        facts = json.loads(remote_facts.read_text())
        assert facts['public_origin'] == value
        assert origin.scheme == 'https' and origin.port is None
        assert origin.hostname == facts['worker_name'] + '.' + facts['workers_subdomain'] + '.workers.dev'
    return origin


def credential(path):
    metadata = path.stat()
    assert not path.is_symlink() and stat.S_ISREG(metadata.st_mode)
    assert stat.S_IMODE(metadata.st_mode) == 0o600
    assert metadata.st_uid == os.getuid() and metadata.st_size <= 8192
    return path.read_text().strip()


def record(path, value):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        json.dump(value, output, indent=2)
        output.write('\n')
        output.flush()
        os.fsync(output.fileno())


def call(origin, path, token=None, payload=None, expected_subject=None):
    headers = {'Content-Type': 'application/json',
               'User-Agent': 'lenso-qualification/1.0'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    if expected_subject:
        headers['X-Lenso-Expected-Subject'] = expected_subject
    request = Request(origin + path, headers=headers,
        data=None if payload is None else json.dumps(payload).encode())
    try:
        response = urlopen(request, timeout=30)
    except HTTPError as failure:
        response = failure
    with response:
        assert response.headers.get('Cache-Control') == 'no-store'
        return response.status, json.load(response)


def mutation(directory, phase, origin, path, token, payload, expected_subject=None):
    # A lost reply leaves this marker; a repeated verifier cannot dispatch again.
    record(directory / (phase + '.started.json'), {'path': path, 'request': payload})
    response = call(origin, path, token, payload, expected_subject)
    record(directory / (phase + '.completed.json'), {
        'status': response[0], 'response': response[1]})
    return response


def read(origin, token, entry):
    request = {'entry_id': entry['id'], 'version': entry['version'],
        'input_json': '{}', 'expected_revision': None, 'idempotency_key': None}
    status, response = call(origin, '/management/invoke', token, request)
    assert status == 200 and response['state'] == 'succeeded'
    assert response['operation_id'] is None
    return json.loads(response['result_json'])


def journey(origin, tokens, directory, nonce, infrastructure='local-workerd'):
    assert call(origin, '/management/catalog')[0] == 401
    status, catalog = call(origin, '/management/catalog', tokens['alice'])
    assert status == 200
    assert {entry['id'] for entry in catalog['entries']} == {'state.read', 'state.update'}
    entries = {entry['id']: entry for entry in catalog['entries']}
    assert entries['state.update']['requires_approval'] is True
    assert entries['state.update']['target_instance'] == 'example.ops-state/primary'
    initial = read(origin, tokens['alice'], entries['state.read'])
    command = {'entry_id': 'state.update', 'version': entries['state.update']['version'],
        'input_json': '{"value":47}', 'expected_revision': str(initial['revision']),
        'idempotency_key': 'workers-reviewed-' + nonce}
    if 'wrong_deployment' in tokens:
        status, hidden = call(origin, '/management/catalog', tokens['wrong_deployment'])
        assert status == 200 and hidden['entries'] == []
        status, _ = mutation(directory, 'wrong-deployment-write-denied', origin,
            '/management/invoke', tokens['wrong_deployment'], command)
        assert status == 403
        assert read(origin, tokens['alice'], entries['state.read']) == initial
    status, changed_session = mutation(directory, 'subject-mismatch', origin,
        '/management/invoke', tokens['alice'], command, expected_subject='bob')
    assert status == 412 and changed_session['code'] == 'session_changed'
    assert read(origin, tokens['alice'], entries['state.read']) == initial
    status, pending = mutation(directory, 'write-pending', origin,
        '/management/invoke', tokens['alice'], command)
    assert status == 200 and pending['state'] == 'pending_approval'
    operation = pending['operation_id']
    assert isinstance(operation, str) and operation
    if 'wrong_deployment' in tokens:
        assert call(origin, '/management/operations/' + operation,
                    tokens['wrong_deployment'])[0] == 403
    assert read(origin, tokens['alice'], entries['state.read']) == initial
    status, intent = call(origin, '/management/approval/' + operation, tokens['bob'])
    assert status == 200 and intent['requester'] == 'alice' and intent['status'] == 'pending'
    assert intent['operation_id'] == operation and intent['entry_id'] == 'state.update'
    assert intent['deployment'] == catalog['deployment']
    assert json.loads(intent['parameters_json']) == {
        'input': {'value': 47}, 'expected_revision': str(initial['revision'])}
    decision = {'operation_id': operation, 'intent_digest': intent['intent_digest'],
        'decision': 'approved'}
    for principal in ['alice', 'machine']:
        status, _ = mutation(directory, principal + '-decision-denied', origin,
            '/management/approval', tokens[principal], decision)
        assert status == 403
    status, _ = mutation(directory, 'changed-intent-denied', origin,
        '/management/approval', tokens['bob'], {**decision, 'intent_digest': '0' * 64})
    assert status == 409
    status, approved = mutation(directory, 'bob-approved', origin,
        '/management/approval', tokens['bob'], decision)
    assert status == 200 and approved['status'] == 'approved'
    assert approved['audit_pending'] is False
    status, committed = mutation(directory, 'original-commit', origin,
        '/management/invoke', tokens['alice'], command)
    assert status == 200 and committed['state'] == 'succeeded'
    assert committed['operation_id'] == operation and committed['receipt']
    assert committed['audit_pending'] is False
    final = json.loads(committed['result_json'])
    assert final['value'] == 47 and final['revision'] == initial['revision'] + 1
    final_state = {key: final[key] for key in ['label', 'value', 'revision']}
    status, observed = call(origin, '/management/operations/' + operation, tokens['alice'])
    assert status == 200 and observed == committed
    assert read(origin, tokens['bob'], entries['state.read']) == final_state
    record(directory / 'journey.json', {
        'schema': 'lenso.qualification.receipt.v1', 'layer': 'ordinary-source-worker-graph',
        'infrastructure': infrastructure, 'deployment': catalog['deployment'],
        'operation_id': operation, 'intent_digest': intent['intent_digest'],
        'request': command, 'committed': committed, 'final_business_state': final_state,
        'cases': ['fresh_bearer_authentication', 'current_catalog', 'typed_read',
            'expected_subject_predispatch_denial', 'immutable_pending_without_target_write',
            'self_approval_denied', 'real_machine_actor_approval_denied',
            'wrong_intent_denied', 'distinct_human_approved', 'original_operation_commit',
            'durable_status_exact_receipt', 'secret_free_http_no_store'] +
            (['wrong_deployment_hidden_catalog_and_write_status_denied']
             if 'wrong_deployment' in tokens else [])})


def observe(origin, tokens, directory):
    receipt = json.loads((directory / 'journey.json').read_text())
    status, response = call(origin, '/management/operations/' + receipt['operation_id'], tokens['alice'])
    assert status == 200 and response == receipt['committed']
    status, catalog = call(origin, '/management/catalog', tokens['bob'])
    assert status == 200
    entry = next(item for item in catalog['entries'] if item['id'] == 'state.read')
    assert read(origin, tokens['bob'], entry) == receipt['final_business_state']
    record(directory / 'restart-observed.json', {'persistent_restart': 'passed',
        'operation_id': receipt['operation_id'], 'business_state_preserved': True,
        'write_replayed': False})


def revoked(origin, tokens, directory):
    assert call(origin, '/management/catalog', tokens['alice'])[0] == 401
    status, catalog = call(origin, '/management/catalog', tokens['bob'])
    assert status == 200 and catalog['entries']
    record(directory / 'revocation-observed.json', {
        'fresh_owner_credential_revocation': 'passed', 'unrelated_bob_live': True})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=['journey', 'observe', 'revoked'])
    parser.add_argument('--origin', required=True)
    parser.add_argument('--alice-file', type=Path, required=True)
    parser.add_argument('--bob-file', type=Path, required=True)
    parser.add_argument('--machine-file', type=Path, required=True)
    parser.add_argument('--wrong-deployment-file', type=Path)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--nonce', required=True)
    parser.add_argument('--remote-facts', type=Path)
    options = parser.parse_args()
    approved_origin(options.origin, options.remote_facts)
    assert options.nonce.isalnum() and len(options.nonce) <= 32
    options.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    tokens = {name: credential(getattr(options, name + '_file')) for name in ['alice', 'bob', 'machine']}
    if options.wrong_deployment_file:
        tokens['wrong_deployment'] = credential(options.wrong_deployment_file)
    if options.phase == 'journey':
        journey(options.origin, tokens, options.directory, options.nonce,
                'cloudflare-workers' if options.remote_facts else 'local-workerd')
    elif options.phase == 'observe':
        observe(options.origin, tokens, options.directory)
    else:
        revoked(options.origin, tokens, options.directory)
    print(json.dumps({'phase': options.phase, 'result': 'passed'}))


if __name__ == '__main__':
    main()
