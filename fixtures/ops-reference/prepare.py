"""Prepare exact source candidates without relying on sibling checkouts or global CLI upgrades."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile


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


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare_npm_cli(cache, javascript, selections, executable, cli_version):
    source = javascript / 'packages' / 'lenso-cli'
    tracked = command(['git', 'ls-files', '--', 'packages/lenso-cli/package.json',
                       'packages/lenso-cli/bin', 'packages/lenso-cli/README.md'], javascript).splitlines()
    files = [Path(name).relative_to('packages/lenso-cli') for name in tracked]
    if Path('package.json') not in files or Path('bin/lenso.js') not in files \
            or Path('README.md') not in files:
        raise ValueError('The selected SourceJS launcher is missing its tracked entry files')
    if any(not (source / name).is_file() or (source / name).is_symlink() for name in files):
        raise ValueError('SourceJS launcher inputs must be regular tracked files')
    manifest = json.loads((source / 'package.json').read_text())
    if manifest.get('name') != '@lenso/cli' \
            or manifest.get('bin', {}).get('lenso') != 'bin/lenso.js' \
            or manifest.get('dependencies') != {'typescript-parser': 'npm:typescript@5.9.3'}:
        raise ValueError('The selected SourceJS launcher has an unexpected package or parser dependency')
    platform = json.loads(command(['node', '-p',
                                  'JSON.stringify({platform:process.platform,arch:process.arch})']))
    tag = platform['platform'] + '-' + platform['arch']
    if tag not in {'darwin-arm64', 'darwin-x64', 'linux-x64', 'win32-x64'}:
        raise ValueError(f'The SourceJS launcher does not support this platform: {tag}')
    binary = Path('vendor') / tag / ('lenso.exe' if platform['platform'] == 'win32' else 'lenso')
    identity = {'schema': 'lenso.ops-source-npm-cli.v1',
                'qualification': 'one_platform_source_launcher_not_npm_release',
                'javascript': selections['javascript'], 'core': selections['core'],
                'package_name': manifest['name'], 'package_version': manifest['version'],
                'native_version': cli_version, 'native_sha256': sha256(executable),
                'platform_tag': tag,
                'source_files_sha256': {name.as_posix(): sha256(source / name) for name in files}}

    def verify(directory, receipt=None):
        for name, digest in identity['source_files_sha256'].items():
            path = directory / name
            if not path.is_file() or path.is_symlink() or sha256(path) != digest:
                raise ValueError('The npm-cli stage differs from its tracked SourceJS selection')
        native = directory / binary
        if not native.is_file() or native.is_symlink() or sha256(native) != identity['native_sha256']:
            raise ValueError('The npm-cli stage belongs to another native executable')
        lock_path = directory / 'package-lock.json'
        if not lock_path.is_file() or lock_path.is_symlink():
            raise ValueError('The npm-cli stage has no regular parser lock')
        lock = json.loads(lock_path.read_text())
        parser = lock.get('packages', {}).get('node_modules/typescript-parser', {})
        if lock.get('packages', {}).get('', {}).get('dependencies') != manifest['dependencies'] \
                or parser.get('version') != '5.9.3' or not isinstance(parser.get('integrity'), str):
            raise ValueError('The npm-cli parser lock differs from the exact runtime dependency')
        installed = directory / 'node_modules' / 'typescript-parser' / 'package.json'
        installed_manifest = json.loads(installed.read_text())
        if installed_manifest.get('name') != 'typescript' or installed_manifest.get('version') != '5.9.3':
            raise ValueError('The npm-cli stage has no matching installed TypeScript parser')
        launcher = directory / 'bin' / 'lenso.js'
        if platform['platform'] != 'win32' and launcher.stat().st_mode & 0o111 == 0:
            raise ValueError('The npm-cli launcher is not executable')
        if command(['node', str(launcher), '--version']) != cli_version:
            raise ValueError('The SourceJS launcher does not run the selected native CLI')
        digests = {'launcher_sha256': sha256(launcher), 'parser_lock_sha256': sha256(lock_path)}
        if receipt is not None and any(receipt.get(key) != value for key, value in digests.items()):
            raise ValueError('The completed npm-cli stage has changed; choose a fresh --cache')
        return digests

    directory = cache / 'npm-cli'
    if directory.exists() or directory.is_symlink():
        receipt_path = directory / 'source-stage.json'
        if directory.is_symlink() or not receipt_path.is_file() or receipt_path.is_symlink():
            raise ValueError('Existing npm-cli stage has no completed Source receipt; choose a fresh --cache')
        receipt = json.loads(receipt_path.read_text())
        if any(receipt.get(key) != value for key, value in identity.items()):
            raise ValueError('Existing npm-cli stage belongs to another source/native selection; choose a fresh --cache')
        verify(directory, receipt)
    else:
        staging = Path(tempfile.mkdtemp(prefix='.npm-cli-', dir=cache))
        for name in files:
            destination = staging / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source / name, destination)
        (staging / binary).parent.mkdir(parents=True)
        shutil.copy2(executable, staging / binary)
        launcher = staging / 'bin' / 'lenso.js'
        launcher.chmod(launcher.stat().st_mode | 0o111)
        try:
            subprocess.run(['npm', 'install', '--ignore-scripts', '--omit=dev',
                            '--no-audit', '--no-fund'], cwd=staging, check=True)
            receipt = {**identity, **verify(staging)}
            (staging / 'source-stage.json').write_text(json.dumps(receipt, indent=2) + '\n')
            staging.rename(directory)
        except Exception:
            print(f'Incomplete SourceJS staging retained at {staging}; no completed cache was replaced', file=sys.stderr)
            raise
    return {'npm_cli': str(directory / 'bin' / 'lenso.js'), 'npm_cli_source': receipt}


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, required=True, help='New or matching candidate-only tool directory')
    args = parser.parse_args()
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    selections = json.loads((Path(__file__).parent / 'candidate-inputs.json').read_text())
    evidence_path = cache / 'tools.json'
    if evidence_path.is_symlink():
        raise ValueError('The tools receipt must be a regular cache file')
    if evidence_path.exists() and json.loads(evidence_path.read_text()).get('selections') != selections:
        raise ValueError('Existing tools receipt belongs to another source selection; choose a fresh --cache')
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
    if executable.exists() or executable.is_symlink():
        if executable.is_symlink() or sha256(executable) != sha256(compiler_executable):
            raise ValueError('Existing CLI cache belongs to another native executable; choose a fresh --cache')
    else:
        shutil.copy2(compiler_executable, executable)
    cli_version = command([str(executable), '--version'])
    if cli_version != 'lenso '+selections['core']['native_cli_version']:
        raise ValueError('Built CLI version differs from its exact source selection')
    npm_cli = prepare_npm_cli(cache, javascript, selections, executable, cli_version)
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
                **npm_cli,
                'workers_runtime': str(package),
                'workers_runtime_archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest()}
    evidence_path.write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    run()
