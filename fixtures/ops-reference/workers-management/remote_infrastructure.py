"""Create and remove only the six resources recorded for this qualification."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
from uuid import UUID

BINDINGS = ('AUTH_DB', 'ACCESS_CONTROL_DB', 'AUDIT_DB', 'APPROVAL_DB',
            'MANAGEMENT_DB', 'OPS_DB')
FAULT_BINDING = 'AUTH_FAULT_DB'


def private(path):
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode) and not path.is_symlink()
    assert metadata.st_uid == os.getuid() and stat.S_IMODE(metadata.st_mode) == 0o600
    return json.loads(path.read_text())


def record(path, value):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        json.dump(value, output, indent=2)
        output.write('\n')
        output.flush()
        os.fsync(output.fileno())


def wrangler(root, script, phase, arguments):
    """Capture account details privately; never include credential values in arguments."""
    destination = root / (phase + '.command.json')
    assert not destination.exists()
    log = root / (phase + '.wrangler.log')
    descriptor = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.close(descriptor)
    result = subprocess.run(['node', str(script), *arguments], cwd=root,
                            text=True, capture_output=True, check=False,
                            env={**os.environ, 'WRANGLER_LOG_PATH': str(log)})
    record(destination, {'arguments': arguments, 'returncode': result.returncode,
                         'stdout': result.stdout, 'stderr': result.stderr})
    if result.returncode:
        raise RuntimeError('Wrangler failed; inspect the private ' + destination.name)
    return result.stdout


def auth(root, script, phase, expected):
    observed = json.loads(wrangler(root, script, phase, ['whoami', '--json']))
    assert observed['loggedIn'] is True
    assert any(account['id'] == expected for account in observed['accounts'])


def plan(root, prior, whoami):
    assert not root.exists()
    root.mkdir(mode=0o700, parents=True)
    previous = private(prior)
    observed = private(whoami)
    account = previous['account_id']
    assert re.fullmatch(r'[a-f0-9]{32}', account)
    assert observed['loggedIn'] is True
    assert any(entry['id'] == account for entry in observed['accounts'])
    subdomain = previous['workers_dev_subdomain']
    assert re.fullmatch(r'[a-z0-9][a-z0-9-]{0,62}', subdomain)
    nonce = secrets.token_hex(6)
    worker = 'lenso-mgmt-q-' + nonce
    operator = 'lenso-mgmt-operator-q-' + nonce
    value = {'schema': 'lenso.workers-qualification-resource-plan.v1',
             'account_id': account, 'nonce': nonce, 'workers_subdomain': subdomain,
             'worker_name': worker, 'operator_worker_name': operator,
             'deployment': 'ops-management-cf-' + nonce,
             'issuer': 'ops-operators-cf-' + nonce,
             'planned_databases': {binding: 'lenso-mgmt-q-' + nonce + '-' +
                                    binding.lower().replace('_', '-') for binding in BINDINGS}}
    record(root / 'resource-plan.json', value)
    record(root / 'wrangler.infrastructure.jsonc', {'name': worker, 'account_id': account,
             'compatibility_date': '2026-09-26', 'd1_databases': []})
    return {'state': 'planned', 'resources_created': False, 'database_count': len(BINDINGS)}


def create(root, script, binding, local_receipt, read_attempt=1):
    receipt = json.loads(local_receipt.read_text())
    assert receipt['infrastructure'] == 'local-workerd'
    assert receipt['client']['version'] == '2.2.0' and receipt['upstream_commit_calls'] == 1
    assert receipt['lost_client_response'] is True and receipt['target_replayed'] is False
    selected = private(root / 'resource-plan.json')
    assert binding in BINDINGS
    phase = 'create-' + binding.lower()
    assert not (root / (phase + '.started.json')).exists()
    assert 1 <= read_attempt <= 3
    read_phase = phase if read_attempt == 1 else phase + '-read-attempt-' + str(read_attempt)
    auth(root, script, read_phase + '-auth', selected['account_id'])
    configuration = root / 'wrangler.infrastructure.jsonc'
    listed = json.loads(wrangler(root, script, read_phase + '-before',
                                ['d1', 'list', '--json', '--config', str(configuration)]))
    name = selected['planned_databases'][binding]
    assert isinstance(listed, list) and not any(entry['name'] == name for entry in listed)
    record(root / (phase + '.started.json'), {'binding': binding, 'database_name': name,
                                            'before_name_absent': True})
    wrangler(root, script, phase, ['d1', 'create', name, '--binding', binding,
                                   '--update-config', '--config', str(configuration)])
    configured = json.loads(configuration.read_text())
    entries = [entry for entry in configured['d1_databases'] if entry['binding'] == binding]
    assert len(entries) == 1 and entries[0]['database_name'] == name
    UUID(entries[0]['database_id'])
    record(root / (phase + '.completed.json'), entries[0])
    return {'binding': binding, 'state': 'created', 'schema_initialized': False}


def reconcile(root, script, binding):
    selected = private(root / 'resource-plan.json')
    assert binding in BINDINGS
    phase = 'create-' + binding.lower()
    started = private(root / (phase + '.started.json'))
    assert started['before_name_absent'] is True
    assert not (root / (phase + '.completed.json')).exists()
    info = json.loads(wrangler(root, script, phase + '-reconcile',
        ['d1', 'info', started['database_name'], '--json', '--config',
         str(root / 'wrangler.infrastructure.jsonc')]))
    assert info['name'] == started['database_name']
    identifier = info.get('uuid', info.get('id'))
    UUID(identifier)
    record(root / (phase + '.completed.json'), {'binding': binding,
           'database_name': info['name'], 'database_id': identifier,
           'creation_observed_after_unknown': True})
    return {'binding': binding, 'state': 'creation_observed', 'create_replayed': False}


def facts(root):
    selected = private(root / 'resource-plan.json')
    databases = {}
    for binding in BINDINGS:
        item = private(root / ('create-' + binding.lower() + '.completed.json'))
        assert item['binding'] == binding
        assert item['database_name'] == selected['planned_databases'][binding]
        UUID(item['database_id'])
        databases[binding] = {key: item[key] for key in ('database_name', 'database_id')}
    origin = 'https://' + selected['worker_name'] + '.' + selected['workers_subdomain'] + '.workers.dev'
    operator = 'https://' + selected['operator_worker_name'] + '.' + selected['workers_subdomain'] + '.workers.dev'
    result = {key: selected[key] for key in ('deployment', 'issuer', 'account_id', 'nonce',
                                          'worker_name', 'operator_worker_name', 'workers_subdomain')}
    result.update(public_origin=origin, graph_origin=origin, operator_url=operator,
                  d1_bindings=databases)
    record(root / 'deployment-facts.json', result)
    return {'state': 'six_resources_recorded', 'database_count': len(databases),
            'owner_setup': 'not_performed', 'deployment': 'not_performed'}


def create_auth_fault(root, script, remote_receipt, read_attempt=1):
    receipt = json.loads(remote_receipt.read_text())
    assert receipt['infrastructure'] == 'cloudflare-workers'
    assert receipt['client']['version'] == '2.2.0'
    assert receipt['upstream_commit_calls'] == 1 and receipt['target_replayed'] is False
    selected = private(root / 'resource-plan.json')
    name = 'lenso-mgmt-q-' + selected['nonce'] + '-auth-fault'
    phase = 'create-' + FAULT_BINDING.lower()
    assert not (root / (phase + '.started.json')).exists()
    assert 1 <= read_attempt <= 3
    read_phase = phase if read_attempt == 1 else phase + '-read-attempt-' + str(read_attempt)
    auth(root, script, read_phase + '-auth', selected['account_id'])
    cfg = root / 'wrangler.infrastructure.jsonc'
    listed = json.loads(wrangler(root, script, read_phase + '-before',
                                ['d1', 'list', '--json', '--config', str(cfg)]))
    assert not any(entry['name'] == name for entry in listed)
    record(root / (phase + '.started.json'), {'binding': FAULT_BINDING,
           'database_name': name, 'before_name_absent': True, 'owner_setup': 'not_performed'})
    wrangler(root, script, phase, ['d1', 'create', name, '--binding', FAULT_BINDING,
                                 '--update-config', '--config', str(cfg)])
    entries = [entry for entry in private(cfg)['d1_databases']
               if entry['binding'] == FAULT_BINDING]
    assert len(entries) == 1 and entries[0]['database_name'] == name
    UUID(entries[0]['database_id'])
    record(root / (phase + '.completed.json'), entries[0])
    return {'state': 'fault_database_created', 'owner_setup': 'not_performed',
            'original_six_resources_changed': False}


def delete_database(root, script, binding):
    selected = private(root / 'resource-plan.json')
    assert binding in (*BINDINGS, FAULT_BINDING)
    created = private(root / ('create-' + binding.lower() + '.completed.json'))
    assert created['binding'] == binding
    name = ('lenso-mgmt-q-' + selected['nonce'] + '-auth-fault'
            if binding == FAULT_BINDING else selected['planned_databases'][binding])
    assert created['database_name'] == name
    UUID(created['database_id'])
    phase = 'delete-' + binding.lower()
    assert not (root / (phase + '.started.json')).exists()
    auth(root, script, phase + '-auth', selected['account_id'])
    cfg = str(root / 'wrangler.infrastructure.jsonc')
    entries = [entry for entry in private(Path(cfg))['d1_databases']
               if entry['binding'] == binding]
    assert len(entries) == 1
    assert entries[0]['database_name'] == created['database_name']
    assert entries[0]['database_id'] == created['database_id']
    current = json.loads(wrangler(root, script, phase + '-before',
         ['d1', 'info', created['database_id'], '--json', '--config', cfg]))
    assert current['name'] == created['database_name']
    assert current.get('uuid', current.get('id')) == created['database_id']
    record(root / (phase + '.started.json'), created)
    wrangler(root, script, phase, ['d1', 'delete', binding,
                                 '--skip-confirmation', '--config', cfg])
    remaining = json.loads(wrangler(root, script, phase + '-after',
                                   ['d1', 'list', '--json', '--config', cfg]))
    assert not any(item.get('uuid', item.get('id')) == created['database_id'] for item in remaining)
    record(root / (phase + '.completed.json'), {'resource_absence_verified': True})
    return {'binding': binding, 'state': 'deleted_and_absent'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=['plan', 'create', 'create-auth-fault',
                        'reconcile', 'facts', 'delete-database'])
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--wrangler', type=Path)
    parser.add_argument('--binding', choices=(*BINDINGS, FAULT_BINDING))
    parser.add_argument('--prior-account', type=Path)
    parser.add_argument('--whoami', type=Path)
    parser.add_argument('--local-receipt', type=Path)
    parser.add_argument('--read-attempt', type=int, choices=[1, 2, 3], default=1)
    options = parser.parse_args()
    if options.phase == 'plan':
        result = plan(options.root, options.prior_account, options.whoami)
    elif options.phase == 'facts':
        result = facts(options.root)
    else:
        assert options.wrangler.is_file() and options.wrangler.name == 'wrangler.js'
        if options.phase == 'create':
            result = create(options.root, options.wrangler, options.binding, options.local_receipt,
                            options.read_attempt)
        elif options.phase == 'create-auth-fault':
            result = create_auth_fault(options.root, options.wrangler, options.local_receipt,
                                       options.read_attempt)
        elif options.phase == 'reconcile':
            result = reconcile(options.root, options.wrangler, options.binding)
        else:
            result = delete_database(options.root, options.wrangler, options.binding)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
