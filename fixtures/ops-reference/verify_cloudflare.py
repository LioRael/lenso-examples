"""Qualify the ordinary D1 artifact in an authorized, disposable Cloudflare account."""
import argparse
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import time
import urllib.request

from vectors import verify, verify_persistent


def json_array(output):
    decoder = json.JSONDecoder()
    for index, character in enumerate(output):
        if character == '[':
            try:
                value, _ = decoder.raw_decode(output[index:])
            except json.JSONDecodeError:
                continue
            if isinstance(value, list):
                return value
    raise ValueError('Wrangler returned no JSON inventory')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ['cli', 'runtime', 'wrangler', 'wasm-bindgen', 'directory', 'receipt']:
        parser.add_argument('--'+flag, type=Path, required=True)
    parser.add_argument('--account-id', required=True)
    args = parser.parse_args()
    os.environ['LENSO_REFERENCE_HTTP_TRANSPORT'] = 'node'
    root = args.directory.resolve()
    root.mkdir(mode=0o700)
    fixture = Path(__file__).parent.resolve()
    source, artifact = root/'project', root/'distribution'
    shutil.copytree(fixture/'project', source, ignore=shutil.ignore_patterns('.lenso', 'target', 'dist'))
    shutil.rmtree(source/'app/management')
    for plugin in (source/'plugins').iterdir():
        if plugin.is_dir() and plugin.name != 'example.ops-state':
            shutil.rmtree(plugin)
    shutil.copytree(fixture/'storage', root/'storage', ignore=shutil.ignore_patterns('target'))
    for instance in ['primary', 'secondary']:
        path = source/f'plugins/example.ops-state/{instance}.toml'
        path.write_text(path.read_text().replace('simulated', 'workers-d1'))
    facilities = root/'facilities.json'
    facilities.write_text(json.dumps({'schema': 'lenso.host-facilities.v1', 'instances': {
        f'example.ops-state/{instance}': {'state': {'binding': f'OPS_{instance.upper()}',
            'configuration': {'profile': 'workers-d1'}}} for instance in ['primary', 'secondary']}}))
    subprocess.run([str(args.cli), 'app', 'build', '--root', str(source), '--target', 'workers',
        '--wasm-bindgen', str(args.wasm_bindgen), '--workers-runtime', str(args.runtime),
        '--workers-facilities', str(facilities), '--workers-host-limits', str(fixture/'profiles/workers-host-limits.json'),
        '--out', str(artifact)], check=True)
    name = 'lenso-ops-proof-'+secrets.token_hex(6)
    config = json.loads((artifact/'wrangler.jsonc').read_text())
    config.update(name=name, account_id=args.account_id, workers_dev=True, preview_urls=False,
        observability={'enabled': False}, d1_databases=[])
    owned = {'account_id': args.account_id, 'worker': name, 'worker_attempted': False, 'databases': []}
    events = []

    def save():
        (artifact/'wrangler.jsonc').write_text(json.dumps(config, indent=2)+'\n')
        path = root/'owned-resources.json'
        path.write_text(json.dumps(owned, indent=2)+'\n')
        path.chmod(0o600)

    def command(arguments, label):
        process = subprocess.run([str(args.wrangler), *arguments], cwd=artifact, input='y\n',
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            env={**os.environ, 'WRANGLER_SEND_METRICS': 'false', 'WRANGLER_LOG_PATH': str(root/'wrangler.log')})
        path = root/f'{len(events)}-{label}.log'
        path.write_text(process.stdout)
        path.chmod(0o600)
        events.append({'operation': label, 'exit_code': process.returncode})
        if process.returncode:
            raise RuntimeError(f'{label} failed; preserve the owned receipt and reconcile before another mutation')
        return process.stdout

    def ready(url):
        last = None
        for _ in range(24):
            try:
                request = urllib.request.Request(url+'/state/primary', headers={
                    'User-Agent': 'lenso-reference-qualification/1', 'Connection': 'close'})
                with urllib.request.urlopen(request, timeout=5) as response:
                    if response.status == 200:
                        return
                    last = {'http_status': response.status}
            except (OSError, ValueError) as error:
                last = {'error_type': type(error).__name__, 'http_status': getattr(error, 'code', None)}
            time.sleep(2)
        proof['readiness_failure'] = last
        raise RuntimeError('Remote artifact never became ready; no HTTP write was retried')

    save()
    proof = {'schema': 'lenso.ops-cloudflare-qualification.v1', 'status': 'not_run',
        'account_id': args.account_id, 'worker': name, 'events': events, 'cleanup': {},
        'schema_setup_transport': 'bounded_d1_query'}
    try:
        assert args.account_id in command(['whoami'], 'logged-in-account')
        inventory = json_array(command(['d1', 'list', '--json'], 'initial-inventory'))
        for instance, value in [('primary', 11), ('secondary', 29)]:
            database_name = name+'-'+instance
            assert not any(item['name'] == database_name for item in inventory)
            owned['databases'].append({'name': database_name, 'create_attempted': True, 'id': None})
            save()
            command(['d1', 'create', database_name], 'create-'+instance)
            current = json_array(command(['d1', 'list', '--json'], 'read-'+instance))
            database = next(item for item in current if item['name'] == database_name)
            owned['databases'][-1]['id'] = database['uuid']
            config['d1_databases'].append({'binding': 'OPS_'+instance.upper(),
                'database_name': database_name, 'database_id': database['uuid']})
            save()
            setup = root/(instance+'.sql')
            setup.write_text((fixture/'storage/d1/schema.sql').read_text()+
                f"\nINSERT INTO state VALUES(1,'{instance}-state',{value},0);\n")
            command(['d1', 'execute', database_name, '--remote', '--command', setup.read_text()], 'setup-'+instance)
        owned['worker_attempted'] = True
        save()
        output = command(['deploy'], 'deploy')
        import re
        url = re.search(r'https://[^\s]+\.workers\.dev', output).group()
        ready(url)
        proof['cases'] = verify(url)
        proof['build'] = json.loads((artifact/'workers-build.json').read_text())
        proof['url'] = url
        proof['versions'] = [re.search(r'Current Version ID:\s*([a-f0-9-]+)', output).group(1)]
        output = command(['deploy'], 'redeploy-same-artifact')
        ready(url)
        proof['restart_cases'] = verify_persistent(url)
        proof['versions'].append(re.search(r'Current Version ID:\s*([a-f0-9-]+)', output).group(1))
        proof['status'] = 'passed'
        proof['persistent_restart'] = 'passed'
        proof['selections'] = json.loads((fixture/'candidate-inputs.json').read_text())
    except BaseException as error:
        proof['status'] = 'failed'
        proof['failure_type'] = type(error).__name__
        proof['uncertain_http_write_replayed'] = False
        if owned['worker_attempted']:
            try:
                proof['operator_read_only_reconciliation'] = json_array(command(['d1', 'execute',
                    name+'-primary', '--remote', '--json', '--command',
                    'SELECT label,value,revision FROM state; SELECT idempotency_key,value,revision FROM receipts;'],
                    'read-only-business-reconciliation'))
            except (RuntimeError, ValueError):
                proof['operator_read_only_reconciliation'] = 'unavailable'
        raise
    finally:
        try:
            if owned['worker_attempted']:
                command(['delete', name], 'delete-worker')
                result = subprocess.run([str(args.wrangler), 'deployments', 'list', '--name', name],
                    cwd=artifact, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                assert result.returncode != 0 and ('10007' in result.stdout or 'not found' in result.stdout.lower())
                proof['cleanup']['worker_delete_and_absence'] = 'confirmed'
            current = json_array(command(['d1', 'list', '--json'], 'cleanup-inventory'))
            for database in owned['databases']:
                observed = next((item for item in current if item['name'] == database['name']), None)
                if observed:
                    assert database['id'] in [None, observed['uuid']]
                    command(['d1', 'delete', database['name'], '--skip-confirmation'], 'delete-'+database['name'])
            remaining = json_array(command(['d1', 'list', '--json'], 'read-final-inventory'))
            assert not any(item['name'] in [database['name'] for database in owned['databases']] for item in remaining)
            proof['cleanup']['owned_d1_deleted_and_absent'] = 'confirmed'
        finally:
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            args.receipt.write_text(json.dumps(proof, indent=2)+'\n')
    print(json.dumps({'receipt': str(args.receipt), 'status': proof['status'], 'cleanup': proof['cleanup']}))


if __name__ == '__main__':
    main()
