"""Verify a selected ordinary Worker graph that omits the Approval instance."""
import argparse
import json
from pathlib import Path

from verify_graph import approved_origin, call, credential, mutation, read, record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--origin', required=True)
    parser.add_argument('--artifact', type=Path, required=True)
    parser.add_argument('--alice-file', type=Path, required=True)
    parser.add_argument('--bob-file', type=Path, required=True)
    parser.add_argument('--committed-journey', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--remote-facts', type=Path)
    options = parser.parse_args()
    approved_origin(options.origin, options.remote_facts)
    entry = (options.artifact / 'worker.mjs').read_text()
    instances = json.loads(entry.split('const instances = ', 1)[1].split(';\n', 1)[0])
    assert not any(instance['packageId'].startswith('lenso.business-approval')
                   for instance in instances)
    build = json.loads((options.artifact / 'workers-build.json').read_text())
    assert not any(module['package_id'].startswith('lenso.business-approval')
                   for module in build['host_facilities']['owner_modules'])
    options.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    alice = credential(options.alice_file)
    bob = credential(options.bob_file)
    status, catalog = call(options.origin, '/management/catalog', alice)
    assert status == 200 and len(catalog['entries']) == 1
    accepted = catalog['entries'][0]
    assert accepted['id'] == 'state.read' and accepted['effect'] == 'read'
    assert accepted['requires_approval'] is False
    initial = read(options.origin, alice, accepted)
    status, _ = mutation(options.directory, 'write-omitted', options.origin,
        '/management/invoke', alice, {'entry_id': 'state.update',
            'version': accepted['version'], 'input_json': '{"value":48}',
            'expected_revision': str(initial['revision']),
            'idempotency_key': 'workers-readonly-no-approval'})
    assert status == 404
    committed = json.loads(options.committed_journey.read_text())
    status, _ = mutation(options.directory, 'human-decision-omitted', options.origin,
        '/management/approval', bob, {'operation_id': committed['operation_id'],
            'intent_digest': committed['intent_digest'], 'decision': 'approved'})
    assert status == 403
    assert read(options.origin, bob, accepted) == initial
    record(options.directory / 'read-only.json', {
        'schema': 'lenso.qualification.receipt.v1', 'layer': 'ordinary-source-worker-graph',
        'infrastructure': 'cloudflare-workers' if options.remote_facts else 'local-workerd',
        'deployment': catalog['deployment'],
        'profile': 'read-only-without-approval', 'approval_instance': 'absent',
        'approval_facility': 'absent', 'source_digest': build['source_digest'],
        'plan_digest': build['plan_digest'], 'host_wasm_digest': build['host_wasm_digest'],
        'final_business_state': initial, 'cases': ['current_auth_and_read_live',
            'no_approval_runtime_instance_or_private_facility', 'write_entry_not_found',
            'human_decision_denied_before_owner_call', 'business_state_unchanged']})
    print(json.dumps({'profile': 'read-only-without-approval', 'result': 'passed'}))


if __name__ == '__main__':
    main()
