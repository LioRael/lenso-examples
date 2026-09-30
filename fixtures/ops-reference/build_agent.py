"""Build an immutable task executable from the App's exact selected Agent source."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ['source', 'executable', 'receipt']:
        parser.add_argument('--'+flag, type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    selection = tomllib.loads((Path(__file__).parent/'project/app/management/tools/Cargo.toml').read_text())[
        'dependencies']['lenso-agent-management-tools-plugin']
    revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    assert revision == selection['rev'], 'Agent source differs from the App selection'
    assert not subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain',
        '--untracked-files=no'], text=True).strip(), 'Agent source has tracked changes'
    assert not args.executable.exists() and not args.receipt.exists(), 'Preserve an existing immutable build'
    result = subprocess.check_output(['cargo', 'build', '--locked', '--manifest-path', str(source/'Cargo.toml'),
        '-p', 'lenso-agent-management-connection-plugin', '--example', 'management_task',
        '--message-format=json-render-diagnostics'], text=True)
    records = [json.loads(line) for line in result.splitlines() if line.startswith('{')]
    artifacts = [record for record in records if record.get('reason') == 'compiler-artifact'
        and record.get('executable') and record['target']['name'] == 'management_task']
    assert len(artifacts) == 1, 'Expected one actual management_task executable'
    artifact = artifacts[0]
    assert Path(artifact['target']['src_path']).resolve().is_relative_to(source), 'Task source is outside selected checkout'
    args.executable.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(artifact['executable'], args.executable)
    proof = {'schema': 'lenso.agent-task-native-build.v1', 'repository': selection['git'],
        'source_revision': revision, 'target': 'management_task',
        'package': 'lenso-agent-management-connection-plugin',
        'cargo_locked': True, 'tracked_source_clean': True,
        'cargo_lock_sha256': hashlib.sha256((source/'Cargo.lock').read_bytes()).hexdigest(),
        'executable_sha256': hashlib.sha256(args.executable.read_bytes()).hexdigest()}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(proof, indent=2)+'\n')
    print(json.dumps({'receipt': str(args.receipt), 'source_revision': revision}))


if __name__ == '__main__':
    main()
