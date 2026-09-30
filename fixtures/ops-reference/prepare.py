"""Prepare exact source candidates without relying on sibling checkouts or global CLI upgrades."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile


def command(arguments, directory=None):
    return subprocess.check_output(arguments, cwd=directory, text=True).strip()


def checkout(cache, name, selection):
    revision = selection['revision']
    if len(revision) != 40 or any(character not in '0123456789abcdef' for character in revision):
        raise ValueError('Candidate revisions must be complete commit IDs')
    directory = cache / name
    if not directory.exists():
        subprocess.run(['git', 'clone', '--no-checkout', selection['repository'], str(directory)], check=True)
        subprocess.run(['git', 'checkout', '--detach', revision], cwd=directory, check=True)
    if command(['git', 'rev-parse', 'HEAD'], directory) != revision:
        raise ValueError(f'Existing {name} cache belongs to another candidate; choose a fresh --cache')
    if command(['git', 'status', '--porcelain', '--untracked-files=all'], directory):
        raise ValueError(f'Existing {name} cache contains source edits or non-ignored untracked files')
    return directory


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True, help='New or matching candidate-only tool directory')
    args = parser.parse_args()
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    selections = json.loads((Path(__file__).parent / 'candidate-inputs.json').read_text())
    core = checkout(cache, 'lenso', selections['core'])
    javascript = checkout(cache, 'lenso-js', selections['javascript'])
    build = subprocess.Popen(['cargo', 'build', '--locked', '-p', 'lenso-cli',
                              '--message-format=json-render-diagnostics'], cwd=core,
                             stdout=subprocess.PIPE, text=True)
    executable = None
    for line in build.stdout:
        try:
            artifact = json.loads(line)
        except json.JSONDecodeError:
            print(line, end='')
            continue
        if artifact.get('reason') == 'compiler-message':
            print(artifact['message'].get('rendered', ''), file=sys.stderr, end='')
        if artifact.get('reason') == 'compiler-artifact' and artifact.get('executable') \
                and artifact['target']['name'] == 'lenso' and 'bin' in artifact['target']['kind']:
            executable = Path(artifact['executable']).resolve(strict=True)
    if build.wait() != 0 or executable is None:
        raise RuntimeError('The selected Cargo build did not produce the Lenso CLI executable')
    compiler_executable = executable
    executable = cache / 'bin' / 'lenso'
    executable.parent.mkdir(exist_ok=True)
    shutil.copy2(compiler_executable, executable)
    cli_version = command([str(executable), '--version'])
    if cli_version != 'lenso '+selections['core']['native_cli_version']:
        raise ValueError('Built CLI version differs from its exact source selection')
    pack = json.loads(command(['npm', 'pack', './packages/lenso-workers-runtime',
                               '--pack-destination', str(cache), '--json'], javascript))[0]
    archive = cache / pack['filename']
    runtime = cache / 'workers-runtime'
    runtime.mkdir(exist_ok=True)
    with tarfile.open(archive) as bundle:
        bundle.extractall(runtime, filter='data')
    package = runtime / 'package'
    manifest = json.loads((package / 'package.json').read_text())
    if manifest['name'] != '@lenso/workers-runtime' or manifest['version'] != selections['javascript']['runtime_version']:
        raise ValueError('Packed Workers runtime differs from the selected candidate')
    evidence = {'schema': 'lenso.ops-candidate-tools.v1', 'selections': selections,
                'cli': str(executable), 'compiler_executable': str(compiler_executable),
                'cli_version': cli_version,
                'cli_sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
                'workers_runtime': str(package),
                'workers_runtime_archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest()}
    (cache / 'tools.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    run()
