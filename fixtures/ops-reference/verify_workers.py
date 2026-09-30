"""Ordinary source/packed-runtime proof in real local workerd, with explicit D1 emulation."""
import argparse
from contextlib import contextmanager
import json
from pathlib import Path
import queue
import shutil
import signal
import socket
import subprocess
import tempfile
import threading
import time

from vectors import verify, verify_static, verify_persistent


@contextmanager
def worker(wrangler, artifact, persistence, diagnostics):
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    process = subprocess.Popen([str(wrangler), 'dev', '--local', '--port', str(port),
                                '--persist-to', str(persistence), '--show-interactive-dev-session', 'false'],
                               cwd=artifact, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    output, transcript = queue.Queue(), []
    def read():
        for line in process.stdout:
            transcript.append(line)
            output.put(line)
        output.put(None)
    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    try:
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            line = output.get(timeout=max(.1, deadline-time.monotonic()))
            if line is None:
                raise RuntimeError('workerd exited before readiness: ' + ''.join(transcript))
            if 'Ready on http://' in line:
                yield f'http://127.0.0.1:{port}'
                break
        else:
            raise RuntimeError('workerd never reported readiness')
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
        process.wait(timeout=15)
        reader.join(timeout=2)
        # The Wrangler wrapper sends SIGTERM to its child on SIGINT; retain its exit status.
        diagnostics.append({'signal':'SIGINT','wrapper_exit_code':process.returncode})
        assert process.returncode in [0, 1, 130, 143, -signal.SIGINT, -signal.SIGTERM], str(diagnostics)+''.join(transcript)
        with socket.socket() as stopped:
            stopped.settimeout(.5)
            assert stopped.connect_ex(('127.0.0.1', port)) != 0, 'workerd listener remained active'


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--runtime', type=Path, required=True, help='Prepared exact packed runtime directory')
    parser.add_argument('--wrangler', type=Path, required=True)
    parser.add_argument('--wasm-bindgen', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--artifact', type=Path, help='Recheck an already built ordinary distribution')
    args = parser.parse_args()
    cli, wrangler, runtime = [path.resolve(strict=True) for path in [args.cli, args.wrangler, args.runtime]]
    fixture = Path(__file__).parent.resolve()
    with tempfile.TemporaryDirectory(prefix='lenso-ops-workerd-') as directory:
        root = Path(directory)
        source = root / 'project'
        shutil.copytree(fixture/'project', source, ignore=shutil.ignore_patterns('.lenso', 'target', 'dist'))
        shutil.rmtree(source/'app/management', ignore_errors=True)
        for plugin in (source/'plugins').iterdir():
            if plugin.is_dir() and plugin.name != 'example.ops-state':
                shutil.rmtree(plugin)
        shutil.copytree(fixture/'storage', root/'storage', ignore=shutil.ignore_patterns('target'))
        for instance in ['primary', 'secondary']:
            config = source/f'plugins/example.ops-state/{instance}.toml'
            config.write_text(config.read_text().replace('simulated', 'workers-d1'))
        facilities = root/'facilities.json'
        facilities.write_text(json.dumps({'schema':'lenso.host-facilities.v1', 'instances':{
            f'example.ops-state/{instance}': {'state':{'binding':f'OPS_{instance.upper()}',
                'configuration':{'profile':'workers-d1'}}} for instance in ['primary','secondary']}}))
        artifact = root/'distribution'
        if args.artifact:
            shutil.copytree(args.artifact.resolve(strict=True), artifact)
        else:
            subprocess.run([str(cli),'app','build','--root',str(source),'--target','workers',
                        '--wasm-bindgen',str(args.wasm_bindgen),
                        '--workers-runtime',str(runtime),'--workers-facilities',str(facilities),
                        '--workers-host-limits',str(fixture/'profiles/workers-host-limits.json'),
                        '--out',str(artifact)], check=True)
        config = json.loads((artifact/'wrangler.jsonc').read_text())
        config['d1_databases'] = [{'binding':f'OPS_{instance.upper()}', 'database_name':f'ops-{instance}',
                                 'database_id':str(number)*8+'-0000-0000-0000-000000000000'}
                                for number, instance in enumerate(['primary','secondary'], 1)]
        (artifact/'wrangler.jsonc').write_text(json.dumps(config, indent=2)+'\n')
        persistence = root/'local-d1'
        for instance, initial in [('primary',11),('secondary',29)]:
            setup = root/f'{instance}.sql'
            setup.write_text((fixture/'storage/d1/schema.sql').read_text()+
                             f"\nINSERT INTO state VALUES(1,'{instance}-state',{initial},0);\n")
            subprocess.run([str(wrangler),'d1','execute',f'OPS_{instance.upper()}','--local',
                            '--persist-to',str(persistence),'--file',str(setup)], cwd=artifact, check=True)
        diagnostics=[]
        with worker(wrangler, artifact, persistence, diagnostics) as url:
            subprocess.run(['python3',str(fixture/'verify.py'),'--cli',str(cli),'--url',url,
                            '--artifact',str(artifact),'--profile','workers-d1','--receipt',str(args.receipt)], check=True)
        evidence = json.loads(args.receipt.read_text())
        with worker(wrangler, artifact, persistence, diagnostics) as url:
            evidence['restart_cases'] = verify_persistent(url)
        evidence.update(persistent_restart='passed', shutdown='listener_closed', shutdown_diagnostics=diagnostics,
                        wrangler_version=subprocess.check_output([str(wrangler),'--version'],text=True).strip())
        args.receipt.write_text(json.dumps(evidence,indent=2)+'\n')
        print(json.dumps({'receipt':str(args.receipt),'cases':evidence['cases'],
                          'restart_cases':evidence['restart_cases'],'infrastructure':'local-workerd'}))


if __name__ == '__main__':
    run()
