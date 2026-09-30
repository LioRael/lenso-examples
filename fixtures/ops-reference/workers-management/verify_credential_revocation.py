"""Compare one cached MCP read before and after explicit Owner revocation."""
import argparse
import json
from pathlib import Path

from verify_graph import approved_origin, call, credential, read, record
from verify_mcp import owner, rpc


def before(origin, tokens, directory):
    status, catalog = call(origin, '/management/catalog', tokens['alice'])
    assert status == 200
    entry = next(item for item in catalog['entries'] if item['id'] == 'state.read')
    baseline = read(origin, tokens['alice'], entry)
    status, tools = rpc(origin, tokens['mcp_alice'], 'tools/list', {})
    assert status == 200
    selected = next(tool for tool in tools['result']['tools']
                    if tool['annotations'].get('readOnlyHint') and tool['name'] != 'management__status')
    payload = {'name': selected['name'], 'arguments': {'input': {},
                'expected_revision': None, 'idempotency_key': None}}
    status, response = rpc(origin, tokens['mcp_alice'], 'tools/call', payload)
    assert status == 200 and json.loads(owner(response)['result_json']) == baseline
    record(directory / 'before.json', {'original_alice_current_auth': 200,
           'mcp_alice_current_auth_and_cached_tool': 200, 'payload': payload,
           'baseline': baseline, 'credential_values_emitted': False})


def after(origin, tokens, directory):
    previous = json.loads((directory / 'before.json').read_text())
    status, _ = rpc(origin, tokens['mcp_alice'], 'tools/call', previous['payload'])
    assert status == 401
    assert call(origin, '/management/catalog', tokens['alice'])[0] == 401
    status, catalog = call(origin, '/management/catalog', tokens['bob'])
    assert status == 200
    entry = next(item for item in catalog['entries'] if item['id'] == 'state.read')
    assert read(origin, tokens['bob'], entry) == previous['baseline']
    record(directory / 'after.json', {'same_cached_tool_payload_current_auth_denied': 401,
           'original_alice_current_auth_denied': 401, 'bob_current_auth_control': 200,
           'business_state_preserved': previous['baseline'], 'target_write_calls': 0})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=['before', 'after'])
    parser.add_argument('--origin', required=True)
    parser.add_argument('--alice-file', type=Path, required=True)
    parser.add_argument('--mcp-alice-file', type=Path, required=True)
    parser.add_argument('--bob-file', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--remote-facts', type=Path)
    options = parser.parse_args()
    approved_origin(options.origin, options.remote_facts)
    options.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    tokens = {name: credential(getattr(options, name + '_file'))
              for name in ['alice', 'mcp_alice', 'bob']}
    if options.phase == 'before':
        before(options.origin, tokens, options.directory)
    else:
        after(options.origin, tokens, options.directory)
    print(json.dumps({'phase': options.phase, 'result': 'passed', 'owner_mutations': False}))


if __name__ == '__main__':
    main()
