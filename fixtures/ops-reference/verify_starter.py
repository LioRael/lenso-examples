"""Build the ordinary generated Web App and start its artifact without source."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import urllib.request

from evidence import native_build
from host import native_server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--source-cohort', action='store_true',
                        help='Select the recorded Git SDK candidate explicitly; this is not registry-only proof')
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='lenso-generated-web-starter-') as directory:
        root = Path(directory)
        source, target = root/'project', root/'distribution'
        subprocess.run([str(cli), 'app', 'create', str(source), '--web', '--no-install'], check=True)
        manifests = sorted(source.rglob('Cargo.toml'))
        if not manifests:
            raise ValueError('The generated Web starter has no Rust Plugin Root')
        original_manifests = {str(manifest.relative_to(source)):
                              hashlib.sha256(manifest.read_bytes()).hexdigest()
                              for manifest in manifests}
        selection = None
        if args.source_cohort:
            fixture = Path(__file__).parent
            selection = json.loads((fixture/'candidate-inputs.json').read_text())['core']
            patches = tomllib.loads((fixture/'project/Cargo.toml').read_text())['patch']['crates-io']
            core = {name: value for name, value in patches.items()
                    if value.get('git') == 'https://github.com/LioRael/lenso'}
            if not core or any(set(value) != {'git', 'rev'} or
                               value['rev'] != selection['revision'] for value in core.values()):
                raise ValueError('The explicit SDK cohort and recorded source patches disagree')
            for manifest in manifests:
                created = tomllib.loads(manifest.read_text())
                if created.get('patch'):
                    raise ValueError('The generated starter already selected a package patch')
                with manifest.open('a') as output:
                    output.write('\n[patch.crates-io]\n')
                    for name, value in sorted(core.items()):
                        output.write(f'{name} = {{ git = "{value["git"]}", rev = "{value["rev"]}" }}\n')
        subprocess.run([str(cli), 'app', 'build', '--root', str(source), '--out', str(target)], check=True)
        subprocess.run([str(cli), 'app', 'check', '--root', str(target)], check=True)
        build = native_build(target)
        shutil.rmtree(source)
        with native_server(cli, target, None, root) as url:
            with urllib.request.urlopen(url, timeout=5) as response:
                assert response.status == 200 and '<html' in response.read().decode().lower()
        proof = {'schema': 'lenso.generated-web-starter-qualification.v1',
            'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
            'cli_version': subprocess.check_output([str(cli), '--version'], text=True).strip(),
            'sdk_selection': 'explicit_source_cohort' if args.source_cohort else 'registry_only',
            'source_cohort': selection,
            'generated_manifest_sha256': original_manifests,
            'build': build, 'source_deleted': True,
            'cases': ['ordinary_app_create_web', 'ordinary_app_build_check', 'offline_native_html_200'],
            'shutdown': 'passed'}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(proof, indent=2)+'\n')
    print(json.dumps({'receipt': str(args.receipt), 'cases': proof['cases']}))


if __name__ == '__main__':
    main()
