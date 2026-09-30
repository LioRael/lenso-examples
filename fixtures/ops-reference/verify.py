"""Build an ordinary source App and exercise its offline HTTP distribution."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from vectors import verify, verify_static, verify_persistent
from host import native_server
from evidence import native_build


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', required=True, help='Exact native Lenso executable')
    parser.add_argument('--url', help='Existing fresh Workers HTTP deployment; skip Native build')
    parser.add_argument('--static', action='store_true', help='G1 routing only')
    parser.add_argument('--profile', choices=['simulated', 'workers-d1'], default='simulated')
    parser.add_argument('--infrastructure', choices=['local-workerd', 'cloudflare'], default='local-workerd')
    parser.add_argument('--artifact', type=Path, help='Workers distribution whose build receipt and digests are retained')
    parser.add_argument('--after-restart', action='store_true', help='Query prior receipts after recreating the Host')
    parser.add_argument('--receipt', type=Path, help='Write the observed test evidence')
    args = parser.parse_args()
    cli = Path(args.cli).resolve(strict=True)
    version = subprocess.check_output([str(cli), '--version'], text=True).strip()
    evidence = {'profile': args.profile, 'cli_version': version,
                'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                'real_database': args.profile == 'workers-d1' and args.infrastructure == 'cloudflare',
                'persistent_restart': 'not_run',
                'auth_and_management': 'not_run'}
    if args.url:
        if not args.artifact:
            parser.error('--url requires --artifact to retain the ordinary build evidence')
        build = json.loads((args.artifact / 'workers-build.json').read_text())
        subprocess.run([str(cli), 'app', 'explain', '--root', str(args.artifact), '--json'], check=True)
        digest = 'sha256:' + hashlib.sha256((args.artifact / 'host_bg.wasm').read_bytes()).hexdigest()
        assert digest == build['host_wasm_digest']
        evidence.update(target='workers_http', build=build, url=args.url, infrastructure=args.infrastructure,
                        selections=json.loads((Path(__file__).parent / 'candidate-inputs.json').read_text()))
        cases = verify_persistent if args.after_restart else verify_static if args.static else verify
        evidence['cases'] = cases(args.url)
        if args.after_restart:
            evidence['persistent_restart'] = 'passed'
    else:
        if args.profile != 'simulated' or args.after_restart:
            parser.error('use verify_pg.py for a real Native PostgreSQL profile')
        with tempfile.TemporaryDirectory(prefix='lenso-ops-consumer-') as directory:
            root = Path(directory)
            source = root / 'source'
            shutil.copytree(Path(__file__).parent / 'project', source,
                            ignore=shutil.ignore_patterns('.lenso', 'target', 'dist'))
            shutil.rmtree(source / 'app/management', ignore_errors=True)
            for plugin in (source / 'plugins').iterdir():
                if plugin.is_dir() and plugin.name != 'example.ops-state':
                    shutil.rmtree(plugin)
            distribution = root / 'distribution'
            shutil.copytree(Path(__file__).parent / 'storage', root / 'storage', ignore=shutil.ignore_patterns('target'))
            facilities = root / 'facilities.json'
            shutil.copyfile(Path(__file__).parent / 'profiles/simulated-native.json', facilities)
            contract = source / 'contracts/example.ops-state-v2'
            (contract / 'src/generated.rs').unlink(missing_ok=True)
            (contract / 'capability.json').unlink(missing_ok=True)
            shutil.rmtree(contract / 'schemas', ignore_errors=True)
            subprocess.run([str(cli), 'app', 'build', '--root', str(source),
                            '--out', str(distribution)], check=True)
            assert (contract / 'src/generated.rs').is_file()
            subprocess.run([str(cli), 'app', 'check', '--root', str(distribution)], check=True)
            subprocess.run([str(cli), 'app', 'show', '--root', str(distribution)], check=True)
            evidence['build'] = native_build(distribution)
            shutil.rmtree(source)
            shutil.rmtree(root / 'storage')
            with native_server(cli, distribution, facilities, root) as url:
                evidence.update(target='native_http', source_deleted=True, cases=verify(url))
            evidence['shutdown'] = 'passed'
    if args.receipt:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    run()
