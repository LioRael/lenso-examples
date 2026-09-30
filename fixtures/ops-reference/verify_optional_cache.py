"""Prove a saved optional absence through ordinary source builds and Host restart."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from evidence import native_build
from host import native_server, rejected_native_start
from vectors import request_path


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    fixture = Path(__file__).parent.resolve()
    with tempfile.TemporaryDirectory(prefix='lenso-ops-optional-cache-') as temporary:
        root = Path(temporary)
        source = root/'project'
        shutil.copytree(fixture/'project', source, ignore=shutil.ignore_patterns('.lenso', 'target', 'dist'))
        shutil.rmtree(source/'app/management')
        for plugin in (source/'plugins').iterdir():
            if plugin.is_dir() and plugin.name != 'example.ops-state':
                shutil.rmtree(plugin)
        shutil.copytree(fixture/'storage', root/'storage', ignore=shutil.ignore_patterns('target'))
        facilities = root/'facilities.json'
        facilities.write_text((fixture/'profiles/simulated-native.json').read_text())
        choices = source/'plugins/.dependencies.json'
        saved = choices.read_bytes()
        selection = next(choice for choice in json.loads(saved)['choices'] if choice['requirement_id'] == 'cache')
        assert selection['provider'] is None

        def build(artifact):
            subprocess.run([str(args.cli), 'app', 'build', '--root', str(source), '--out', str(artifact)], check=True)

        def inspect(artifact):
            configurations = [choices, *(source/'plugins/example.ops-state').glob('*.toml')]
            before = {path: hashlib.sha256(path.read_bytes()).hexdigest() for path in configurations}
            for command in ['check', 'show', 'explain']:
                options = ['--host-facilities', str(facilities), '--json'] if command == 'explain' else []
                subprocess.run([str(args.cli), 'app', command, '--root', str(artifact), *options], check=True)
            assert before == {path: hashlib.sha256(path.read_bytes()).hexdigest() for path in configurations}
            assert choices.read_bytes() == saved

        def observe(artifact):
            with native_server(args.cli, artifact, facilities, root) as url:
                assert request_path(url, '/cache') == (200, {'enabled': False})
                assert request_path(url, '/state/primary')[1]['value'] == 11
                assert request_path(url, '/state/secondary')[1]['value'] == 29

        first = root/'before-candidate'
        build(first)
        inspect(first)
        observe(first)
        for name, invalid in [('workers_binding_on_native', {'binding': 'OPS_PRIMARY', 'configuration': {'profile': 'workers-d1'}}),
                              ('kv_cannot_supply_authority_state', {'profile': 'kv', 'binding': 'CACHE'})]:
            grants = json.loads(facilities.read_text())
            grants['instances']['example.ops-state/primary']['state'] = invalid
            rejected = root/(name+'.json')
            rejected.write_text(json.dumps(grants))
            rejected_native_start(args.cli, first, rejected, root, 'invalid Ops State Host facility')
        (source/'plugins/example.ops-state/cache.toml').write_text('label = "cache-candidate"\ninitial_value = 99\nprofile = "simulated"\n')
        grants = json.loads(facilities.read_text())
        grants['instances']['example.ops-state/cache'] = {'state': {'profile': 'simulated'}}
        facilities.write_text(json.dumps(grants))
        final = root/'after-candidate'
        build(final)
        inspect(final)
        observe(final)
        evidence = {'schema': 'lenso.ops-optional-cache-qualification.v1', 'build': native_build(final),
                    'target': 'native_http', 'real_database': False, 'saved_provider': None,
                    'cases': ['saved_explicit_none', 'new_configured_provider_does_not_replace_none',
                              'inspection_preserves_configuration', 'host_restart_preserves_none',
                              'workers_binding_on_native_rejected', 'kv_authority_profile_rejected']}
        shutil.rmtree(source)
        shutil.rmtree(root/'storage')
        observe(final)
        evidence.update(source_deleted=True, shutdown='passed')
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(evidence, indent=2)+'\n')
    print(json.dumps({'receipt': str(args.receipt), 'cases': evidence['cases']}))


if __name__ == '__main__':
    run()
