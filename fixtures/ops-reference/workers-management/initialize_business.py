"""Initialize only the new OPS_DB recorded for this remote qualification."""
import argparse
import hashlib
import json
import os
from pathlib import Path

from remote_infrastructure import auth, private, record, wrangler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--wrangler', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    facts = private(root/'deployment-facts.json')
    created = private(root/'create-ops_db.completed.json')
    assert facts['d1_bindings']['OPS_DB'] == {key:created[key]
        for key in ['database_name', 'database_id']}
    assert created['database_name'].startswith('lenso-mgmt-q-'+facts['nonce']+'-')
    assert not (root/'ops-schema.started.json').exists(), 'Reconcile an attempted initialization before continuing'
    auth(root, args.wrangler, 'ops-schema-auth', facts['account_id'])
    configuration = str(root/'wrangler.infrastructure.jsonc')
    target = created['database_id']
    command = ['d1', 'execute', target, '--remote', '--json', '--config', configuration]
    before = json.loads(wrangler(root, args.wrangler, 'ops-schema-before', command+[
        '--command', "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('schema_version','state','receipts');"]))
    assert before and all(item['success'] and not item['results'] for item in before)
    schema = Path(__file__).resolve().parents[1]/'storage/d1/schema.sql'
    sql = schema.read_text()+"\nINSERT INTO state VALUES (1, 'primary-state', 11, 0);\n"
    sql_path = root/'ops-schema.sql'
    descriptor = os.open(sql_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        file.write(sql)
    record(root/'ops-schema.started.json', {'binding':'OPS_DB', 'database_id':target,
        'owner_schema_sha256':hashlib.sha256(schema.read_bytes()).hexdigest(),
        'sql_sha256':hashlib.sha256(sql.encode()).hexdigest(), 'owner_tables_absent_before':True})
    wrangler(root, args.wrangler, 'ops-schema-initialize', command+['--file', str(sql_path), '--yes'])
    after = json.loads(wrangler(root, args.wrangler, 'ops-schema-after', command+[
        '--command', 'SELECT label, value, revision FROM state WHERE singleton=1;']))
    assert len(after)==1 and after[0]['success']
    assert after[0]['results']==[{'label':'primary-state','value':11,'revision':0}]
    record(root/'ops-schema.completed.json', {'binding':'OPS_DB', 'database_id':target,
        'state':after[0]['results'][0], 'owner_setup_repeated':False})
    print(json.dumps({'binding':'OPS_DB','state':'initialized_and_read_verified',
        'initial_value':11,'initial_revision':0,'owner_setup_repeated':False}))


if __name__ == '__main__':
    main()
