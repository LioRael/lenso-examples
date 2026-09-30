"""Attach a reviewed remote graph to its six newly recorded D1 resources."""
import argparse
import json
from pathlib import Path
import re
from uuid import UUID

from remote_infrastructure import BINDINGS, private, record


def prepare(output, facts_path, secrets_path, secret_upload):
    facts = private(facts_path)
    assert set(facts['d1_bindings']) == set(BINDINGS)
    assert re.fullmatch(r'[a-f0-9]{32}', facts['account_id'])
    expected = 'https://' + facts['worker_name'] + '.' + facts['workers_subdomain'] + '.workers.dev'
    assert facts['graph_origin'] == facts['public_origin'] == expected
    bindings = []
    for name in BINDINGS:
        item = facts['d1_bindings'][name]
        assert set(item) == {'database_name', 'database_id'}
        UUID(item['database_id'])
        assert item['database_name'].startswith('lenso-mgmt-q-' + facts['nonce'] + '-')
        bindings.append({'binding': name, **item})
    values = private(secrets_path)
    assert set(values) == {'operators/api-signing', 'operators/api-pepper'}
    assert all(isinstance(value, str) and 32 <= len(value) <= 16384 for value in values.values())
    assert not (output / '.dev.vars').exists()
    configuration_path = output / 'wrangler.jsonc'
    configuration = json.loads(configuration_path.read_text())
    assert configuration['main'] == 'worker.mjs'
    assert not configuration.get('routes')
    configuration.update(name=facts['worker_name'], account_id=facts['account_id'],
        workers_dev=True, preview_urls=False, observability={'enabled': False},
        d1_databases=bindings, vars={'MANAGEMENT_MCP_PROFILE': 'selected'})
    configuration_path.write_text(json.dumps(configuration, indent=2) + '\n')
    record(secret_upload, {'OPS_AUTH_SECRETS_JSON': json.dumps(values, separators=(',', ':'))})
    return {'artifact': str(output), 'binding_count': len(bindings),
            'secret_upload_file': str(secret_upload), 'secret_values_emitted': False,
            'owner_setup': 'not_performed', 'deployment': 'not_performed'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifact', type=Path, required=True)
    parser.add_argument('--facts', type=Path, required=True)
    parser.add_argument('--secrets', type=Path, required=True)
    parser.add_argument('--secret-upload-file', type=Path, required=True)
    options = parser.parse_args()
    print(json.dumps(prepare(options.artifact, options.facts, options.secrets,
                             options.secret_upload_file)))


if __name__ == '__main__':
    main()
