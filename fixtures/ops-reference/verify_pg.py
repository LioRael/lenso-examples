"""Real PostgreSQL HTTP proof with explicit operator setup and two Host generations."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from urllib.parse import parse_qsl, unquote, urlsplit

from host import native_server, rejected_native_start
from vectors import verify, verify_persistent
from evidence import native_build


def sql(uri, text):
    parsed = urlsplit(uri)
    if parsed.scheme not in ['postgres', 'postgresql'] or not parsed.hostname:
        raise ValueError('The test environment must use an explicit PostgreSQL URI')
    environment = {**os.environ, 'PGHOST': parsed.hostname, 'PGPORT': str(parsed.port or 5432),
                   'PGDATABASE': unquote(parsed.path.lstrip('/')),
                   'PGUSER': unquote(parsed.username or ''), 'PGPASSWORD': unquote(parsed.password or '')}
    parameters = dict(parse_qsl(parsed.query))
    if 'sslmode' in parameters:
        environment['PGSSLMODE'] = parameters['sslmode']
    subprocess.run(['psql', '--no-psqlrc', '--set', 'ON_ERROR_STOP=1', '--quiet'],
                   input=text, text=True, check=True, env=environment)


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', required=True)
    parser.add_argument('--database-url-file', type=Path, help='Authorized test URI file; keep credentials out of command arguments')
    parser.add_argument('--receipt', type=Path)
    args = parser.parse_args()
    cli = Path(args.cli).resolve(strict=True)
    uri = args.database_url_file.read_text().strip() if args.database_url_file else os.environ.get('LENSO_OPS_TEST_DATABASE_URL')
    if not uri:
        parser.error('Set an authorized LENSO_OPS_TEST_DATABASE_URL')
    fixture = Path(__file__).parent
    schemas = []
    try:
        with tempfile.TemporaryDirectory(prefix='lenso-ops-pg-consumer-') as directory:
            root = Path(directory)
            source, distribution = root / 'source', root / 'distribution'
            shutil.copytree(fixture / 'project', source,
                            ignore=shutil.ignore_patterns('.lenso', 'target', 'dist'))
            shutil.rmtree(source / 'app/management', ignore_errors=True)
            for plugin in (source / 'plugins').iterdir():
                if plugin.is_dir() and plugin.name != 'example.ops-state':
                    shutil.rmtree(plugin)
            shutil.copytree(fixture / 'storage', root / 'storage', ignore=shutil.ignore_patterns('target'))
            secret = root / 'postgres-uri'
            secret.write_text(uri)
            secret.chmod(0o600)
            bindings = {}
            for instance, initial in [('primary', 11), ('secondary', 29)]:
                schema = f'ops_http_{os.getpid()}_{instance}'
                sql(uri, f'BEGIN; CREATE SCHEMA {schema}; SET search_path TO {schema};\n'
                    + (fixture / 'storage/native-pg/schema.sql').read_text()
                    + f"\nINSERT INTO state VALUES(true,'{instance}-state',{initial},0); COMMIT;")
                schemas.append(schema)
                bindings[f'example.ops-state/{instance}'] = {'state': {
                    'profile': 'native-pg', 'connection_uri_file': str(secret), 'schema': schema}}
                config = source / f'plugins/example.ops-state/{instance}.toml'
                config.write_text(config.read_text().replace('simulated', 'native-pg'))
            facilities = root / 'facilities.json'
            facilities.write_text(json.dumps({'schema': 'lenso.host-facilities.v1', 'instances': bindings}))
            subprocess.run([str(cli), 'app', 'build', '--root', str(source), '--out', str(distribution)], check=True)
            subprocess.run([str(cli), 'app', 'check', '--root', str(distribution)], check=True)
            build = native_build(distribution)
            shutil.rmtree(source)
            shutil.rmtree(root / 'storage')
            with native_server(cli, distribution, facilities, root) as url:
                cases = verify(url)
            with native_server(cli, distribution, facilities, root) as url:
                restart_cases = verify_persistent(url)
            # The secondary resource remains valid; deleting primary must not select it.
            missing = schemas[0]
            sql(uri, f'DROP SCHEMA {missing} CASCADE;')
            schemas.remove(missing)
            rejected_native_start(cli, distribution, facilities, root, 'Ops State readiness: SetupRequired')
            cases.append('saved_deleted_resource_rejected_with_valid_secondary')
            evidence = {'target': 'native_http', 'profile': 'native-pg', 'real_database': True,
                        'cli_version': subprocess.check_output([str(cli), '--version'], text=True).strip(),
                        'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(), 'cases': cases,
                        'source_deleted': True, 'persistent_restart': 'passed',
                        'receipt_reconciliation': 'passed', 'restart_cases': restart_cases, 'shutdown': 'passed',
                        'hyperdrive_remote': 'not_run', 'auth_and_management': 'not_run'}
            evidence['build'] = build
            if args.receipt:
                args.receipt.parent.mkdir(parents=True, exist_ok=True)
                args.receipt.write_text(json.dumps(evidence, indent=2) + '\n')
            print(json.dumps(evidence, indent=2))
    finally:
        if schemas:
            sql(uri, '\n'.join(f'DROP SCHEMA {schema} CASCADE;' for schema in schemas))


if __name__ == '__main__':
    run()
