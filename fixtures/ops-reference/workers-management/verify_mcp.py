"""Qualify the selected official MCP transport against the ordinary Worker App."""
import argparse
from http.client import RemoteDisconnected
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import socket
import threading
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from verify_graph import approved_origin, call, credential, mutation, record


def rpc(origin, token, method, parameters, identifier=1):
    packet = {'jsonrpc': '2.0', 'method': method, 'params': parameters}
    if identifier is not None:
        packet['id'] = identifier
    headers = {'Content-Type': 'application/json',
               'User-Agent': 'lenso-qualification/1.0',
        'Accept': 'application/json,text/event-stream',
        'Mcp-Protocol-Version': '2025-11-25', 'Authorization': 'Bearer ' + token}
    try:
        response = urlopen(Request(origin + '/mcp', headers=headers,
            data=json.dumps(packet).encode()), timeout=30)
    except HTTPError as failure:
        response = failure
    with response:
        assert response.headers.get('Cache-Control') == 'no-store'
        body = response.read()
        if response.status == 202:
            return response.status, {}
        if 'text/event-stream' in response.headers.get('Content-Type', ''):
            lines = [line[5:].strip() for line in body.splitlines()
                if line.startswith(b'data:')]
            assert len(lines) == 1
            body = lines[0]
        return response.status, json.loads(body)


def owner(packet):
    reply = packet['result']
    assert not reply.get('isError', False), reply
    return json.loads(next(block['text'] for block in reply['content']
        if block['type'] == 'text'))


def lost_commit(origin, token, parameters, directory):
    """Drop one actual response after privately recording its upstream completion."""
    record(directory / 'commit.started.json', {'method': 'tools/call',
        'request': parameters, 'response_delivery': 'deliberately_dropped'})
    failures = []

    class DropReply(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                assert self.path == '/mcp'
                size = int(self.headers['Content-Length'])
                assert 0 < size <= 262144
                received = json.loads(self.rfile.read(size))
                assert received == {'jsonrpc': '2.0', 'id': 1,
                    'method': 'tools/call', 'params': parameters}
                assert self.headers['Authorization'] == 'Bearer ' + token
                status, packet = rpc(origin, token, 'tools/call', parameters)
                record(directory / 'commit.upstream-completed.json', {
                    'status': status, 'response': packet})
                self.connection.shutdown(socket.SHUT_RDWR)
                self.connection.close()
            except Exception as failure:
                failures.append(type(failure).__name__)
                self.connection.close()

    with HTTPServer(('127.0.0.1', 0), DropReply) as proxy:
        worker = threading.Thread(target=proxy.handle_request, daemon=True)
        worker.start()
        try:
            rpc('http://127.0.0.1:' + str(proxy.server_port), token,
                'tools/call', parameters)
        except (RemoteDisconnected, URLError, ConnectionResetError):
            lost = True
        else:
            lost = False
        worker.join(timeout=35)
        assert lost and not worker.is_alive() and not failures
    upstream = json.loads((directory / 'commit.upstream-completed.json').read_text())
    assert upstream['status'] == 200
    return owner(upstream['response'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--origin', required=True)
    parser.add_argument('--alice-file', type=Path, required=True)
    parser.add_argument('--bob-file', type=Path, required=True)
    parser.add_argument('--machine-file', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--nonce', required=True)
    parser.add_argument('--remote-facts', type=Path)
    options = parser.parse_args()
    approved_origin(options.origin, options.remote_facts)
    assert options.nonce.isalnum() and len(options.nonce) <= 32
    options.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    alice, bob, machine = [credential(getattr(options, name + '_file'))
        for name in ['alice', 'bob', 'machine']]
    status, initialized = rpc(options.origin, alice, 'initialize', {
        'protocolVersion': '2025-11-25', 'capabilities': {},
        'clientInfo': {'name': 'ordinary-worker-qualification', 'version': '1'}})
    assert status == 200 and initialized['result']['protocolVersion'] == '2025-11-25'
    assert rpc(options.origin, alice, 'notifications/initialized', {}, None)[0] == 202
    status, catalog = rpc(options.origin, alice, 'tools/list', {})
    assert status == 200
    tools = catalog['result']['tools']
    assert len(tools) == 3 and all(len(tool['name']) <= 64 for tool in tools)
    assert not any('approve' in tool['name'] or 'token' in tool['name'] for tool in tools)
    read_tool = next(tool for tool in tools if tool['annotations'].get('readOnlyHint')
        and tool['name'] != 'management__status')
    write_tool = next(tool for tool in tools if tool['annotations'].get('destructiveHint'))
    read_args = {'input': {}, 'expected_revision': None, 'idempotency_key': None}
    status, packet = rpc(options.origin, alice, 'tools/call', {
        'name': read_tool['name'], 'arguments': read_args})
    assert status == 200
    initial = json.loads(owner(packet)['result_json'])
    for parameters in [
        {'name': 'hidden_business_write', 'arguments': {}},
        {'name': read_tool['name'], 'arguments': {**read_args,
            'target_instance': 'example.ops-state/secondary'}}]:
        status, rejected = rpc(options.origin, alice, 'tools/call', parameters)
        assert status == 200 and (rejected.get('error') or rejected['result'].get('isError'))
    arguments = {'input': {'value': 49}, 'expected_revision': str(initial['revision']),
        'idempotency_key': 'workers-mcp-lostreply-' + options.nonce}
    parameters = {'name': write_tool['name'], 'arguments': arguments}
    record(options.directory / 'pending.started.json', {'request': parameters})
    status, packet = rpc(options.origin, alice, 'tools/call', parameters)
    record(options.directory / 'pending.completed.json', {'status': status, 'response': packet})
    assert status == 200
    pending = owner(packet)
    assert pending['state'] == 'pending_approval' and pending['operation_id']
    operation = pending['operation_id']
    status, intent = call(options.origin, '/management/approval/' + operation, bob)
    assert status == 200 and json.loads(intent['parameters_json']) == {
        'input': {'value': 49}, 'expected_revision': str(initial['revision'])}
    decision = {'operation_id': operation, 'intent_digest': intent['intent_digest'],
        'decision': 'approved'}
    for name, token in [('self-denied', alice), ('machine-denied', machine)]:
        assert mutation(options.directory, name, options.origin,
            '/management/approval', token, decision)[0] == 403
    status, approved = mutation(options.directory, 'human-approved', options.origin,
        '/management/approval', bob, decision)
    assert status == 200 and approved['status'] == 'approved' and not approved['audit_pending']
    committed = lost_commit(options.origin, alice, parameters, options.directory)
    assert committed['state'] == 'succeeded' and committed['operation_id'] == operation
    status, recovered = rpc(options.origin, alice, 'tools/call', {
        'name': 'management__status', 'arguments': {'operation_id': operation}})
    assert status == 200 and owner(recovered) == committed
    assert call(options.origin, '/management/operations/' + operation, alice) == (200, committed)
    status, final = rpc(options.origin, bob, 'tools/call', {
        'name': read_tool['name'], 'arguments': read_args})
    business = json.loads(owner(final)['result_json'])
    assert status == 200 and business['value'] == 49
    assert business['revision'] == initial['revision'] + 1
    committed_business = json.loads(committed['result_json'])
    assert committed_business['receipt_id'] == committed['receipt']
    assert {key: committed_business[key] for key in ['label', 'value', 'revision']} == business
    record(options.directory / 'mcp-lost-reply.json', {
        'schema': 'lenso.qualification.receipt.v1', 'layer': 'ordinary-source-worker-graph',
        'infrastructure': 'local-workerd', 'transport': 'official-stateless-mcp',
        'operation_id': operation, 'intent_digest': intent['intent_digest'],
        'committed': committed, 'final_business_state': business,
        'upstream_commit_calls': 1, 'lost_client_response': True,
        'recovery': 'status-and-read-only', 'target_replayed': False,
        'cases': ['real_mcp_handshake_catalog_read', 'no_human_or_token_tools',
            'hidden_tool_denied', 'target_override_denied', 'actual_mcp_pending',
            'self_and_machine_approval_denied', 'distinct_human_approved',
            'commit_response_lost', 'same_operation_receipt_recovered_without_invoke']})
    print(json.dumps({'transport': 'official-stateless-mcp', 'result': 'passed'}))


if __name__ == '__main__':
    main()
