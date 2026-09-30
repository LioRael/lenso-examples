"""Qualify optional Native compositions using already initialized owner facts."""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import tempfile
import threading
import time

from evidence import native_build
from verify_human import call, login, profile as human_profile
from verify_management import configuration, private, profile as api_profile, request, prove_mcp

MANAGEMENT = 'example.ops-management'
WEB = 'example.ops-management-web'
TOOLS = ['example.ops-management-tools', 'lenso.agent.management-tools']
MCP = 'example.ops-management-mcp'
APPROVAL = 'lenso.business-approval.postgres'
BOOTSTRAPS = ['example.ops-bootstrap', 'example.ops-human-bootstrap']


def disable(source, plugins):
    for plugin in plugins:
        directory = source / 'plugins' / plugin
        if plugin.startswith('lenso.'):
            if directory.exists():
                shutil.rmtree(directory)
            continue
        directory.mkdir(exist_ok=True)
        for config in directory.glob('*.toml'):
            config.unlink()
        (directory / 'default.disabled').touch()
    dependencies = source / 'plugins/.dependencies.json'
    if dependencies.exists():
        document = json.loads(dependencies.read_text())
        document['choices'] = [choice for choice in document['choices']
            if choice['consumer']['plugin_id'] not in plugins
            and (choice.get('provider') is None or choice['provider']['plugin_id'] not in plugins)]
        dependencies.write_text(json.dumps(document, indent=2) + '\n')


def clone_facts(prepared, destination):
    destination.mkdir()
    for path in prepared.glob('*.secret'):
        private(destination / path.name, path.read_text())
    shutil.copytree(prepared / 'tokens', destination / 'tokens')
    for path in prepared.glob('*.sqlite'):
        shutil.copy2(path, destination / path.name)
    for name in ['operator-input.json', 'setup.json', 'human-setup.json', 'runtime.json']:
        if (prepared / name).exists():
            private(destination / name, (prepared / name).read_text())


def process_tree(parent):
    rows = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,command='], text=True)
    records = {}
    for row in rows.splitlines():
        fields = row.strip().split(None, 2)
        if len(fields) == 3:
            records[int(fields[0])] = (int(fields[1]), fields[2])
    children = {parent}
    while True:
        found = {pid for pid, (ppid, _) in records.items() if ppid in children}
        enlarged = children | found
        if enlarged == children:
            break
        children = enlarged
    return {pid: records[pid][1] for pid in children if pid in records}


@contextmanager
def selected_host(cli, artifact, facilities, directory):
    process = subprocess.Popen([str(cli), 'app', 'start', '--from', str(artifact),
        '--host-facilities', str(facilities)], cwd=directory,
        env={'PATH': str(directory / 'no-toolchains')}, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, text=True)
    lines, messages = [], queue.Queue()
    def collect():
        for line in process.stdout:
            lines.append(line)
            messages.put(line)
        messages.put(None)
    reader = threading.Thread(target=collect, daemon=True)
    reader.start()
    try:
        deadline, url = time.monotonic() + 30, None
        while time.monotonic() < deadline:
            line = messages.get(timeout=max(0.1, deadline - time.monotonic()))
            if line is None:
                raise RuntimeError('Selected Host exited before readiness: ' + ''.join(lines))
            if line.startswith('Listening on '):
                url = line.removeprefix('Listening on ').strip()
            if 'Local App ready' in line:
                break
        else:
            raise TimeoutError('Selected Host did not become ready')
        tree = process_tree(process.pid)
        # The CLI may keep one generated Native Host child; providers may not spawn a model/tool process.
        assert len(tree) <= 2, 'Unexpected runtime subprocess in an optional Native profile'
        assert all(not any(name in command.lower() for name in ['node ', 'bun ', 'python ', 'lenso-agent'])
                   for command in tree.values()), 'Unexpected Agent/model runtime process'
        yield url, tree
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        process.wait(timeout=15)
        reader.join(timeout=2)
        assert process.returncode == 0 and any('Local App stopped cleanly' in line for line in lines), ''.join(lines)


def prepare_case(fixture, root, prepared, name, human=False):
    case = root / name
    case.mkdir()
    facts = case / 'facts'
    clone_facts(prepared, facts)
    source = case / 'project'
    shutil.copytree(fixture / 'project', source,
        ignore=shutil.ignore_patterns('.lenso', 'target', 'dist', 'dist-workers'))
    shutil.copytree(fixture / 'storage', case / 'storage', ignore=shutil.ignore_patterns('target'))
    template_choices = source / 'plugins/.dependencies.json'
    if template_choices.exists():
        (case / 'template-choices.json').write_text(template_choices.read_text())
    inputs = json.loads((facts / 'operator-input.json').read_text())
    setup = json.loads((facts / 'setup.json').read_text())
    if human:
        runtime = json.loads((facts / 'runtime.json').read_text())
        owner = json.loads((facts / 'human-setup.json').read_text())
        facilities = human_profile(source, facts, inputs, setup, owner,
                                   runtime['native_port'], runtime['origin'])
    else:
        facilities = api_profile(source, facts, setup, inputs, inputs['resource_uri'])
    disable(source, BOOTSTRAPS)
    grants = json.loads(facilities.read_text())
    for plugin in BOOTSTRAPS:
        grants['instances'].pop(plugin + '/default', None)
    private(facilities, json.dumps(grants))
    return case, facts, source, facilities, inputs


def build(cli, source, artifact, facilities):
    subprocess.run([str(cli), 'app', 'build', '--root', str(source), '--out', str(artifact)], check=True)
    plan = json.loads(subprocess.check_output([str(cli), 'app', 'show', '--root', str(artifact), '--json'], text=True))
    selected = {instance['id'] for instance in plan['instances']}
    grants = json.loads(facilities.read_text())
    grants['instances'] = {instance: value for instance, value in grants['instances'].items() if instance in selected}
    private(facilities, json.dumps(grants))
    assert not any('model' in instance or 'loop' in instance or 'coding' in instance for instance in selected)
    return selected, native_build(artifact)


def reject_write_without_approval(cli, artifact, facilities, directory):
    process = subprocess.Popen([str(cli), 'app', 'start', '--from', str(artifact),
        '--host-facilities', str(facilities)], cwd=directory, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, text=True, start_new_session=True)
    try:
        output, _ = process.communicate(timeout=30)
        assert process.returncode != 0 and 'Local App ready' not in output \
            and 'write profile requires Approval' in output, \
            'The write profile did not reject the missing Approval binding before readiness'
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=15)


def prove_human(url, facts, baseline):
    runtime = json.loads((facts / 'runtime.json').read_text())
    session = login(url, runtime['origin'], 'alice@ops.test', (facts / 'alice-password.secret').read_text())
    status, reply, _ = call(url, '/api/console/v1/session', session)
    assert status == 200 and reply['authenticated'] is True
    assert call(url, '/management/catalog', session)[0] == 200
    status, read, _ = call(url, '/management/invoke', session, {'entry_id': 'state.read',
        'version': '2.0.0', 'input_json': '{}', 'idempotency_key': None, 'expected_revision': None})
    assert status == 200 and read['state'] == 'succeeded'
    domain = json.loads(read['result_json'])
    assert {key: domain[key] for key in ['value', 'revision']} == baseline, 'Business state did not survive surface removal'
    assert call(url, '/management-tools/catalog')[0] == 404
    assert call(url, '/agent/management/catalog')[0] == 404
    return {'account_and_business_preserved': True, 'agent_routes_absent': True}


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--api-root', type=Path, required=True)
    parser.add_argument('--human-root', type=Path, required=True)
    parser.add_argument('--api-receipt', type=Path, required=True)
    parser.add_argument('--human-receipt', type=Path, required=True)
    parser.add_argument('--agent-receipt', type=Path, required=True,
                        help='Root qualification with actual Auth-issued child and normal Model/Loop Host')
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--directory', type=Path)
    parser.add_argument('--resume-known-profiles', action='store_true',
                        help='Retain completed profile receipts and resume the same prepared facts')
    args = parser.parse_args()
    assert not args.resume_known_profiles or args.directory, 'Resuming requires an explicit prepared directory'
    for receipt in [args.api_receipt, args.human_receipt]:
        assert json.loads(receipt.read_text())['shutdown'] == 'passed', 'Finish the prepared owner proof before optional installation checks'
    human_receipt = json.loads(args.human_receipt.read_text())
    baseline = {key: human_receipt['final_business_state'][key] for key in ['value', 'revision']}
    api_receipt = json.loads(args.api_receipt.read_text())
    api_baseline = {key: api_receipt['final_business_state'][key] for key in ['value', 'revision']}
    agent_receipt = json.loads(args.agent_receipt.read_text())
    assert agent_receipt['fixture_model_loop'] is True
    assert agent_receipt['real_remote_child'] is True, 'A fake transport credential cannot qualify the Agent composition'
    assert agent_receipt['console_human_flow_live_during_agent'] is True
    assert agent_receipt['same_management_process'] is True
    assert {key: agent_receipt['final_business_state'][key] for key in ['value', 'revision']} == baseline
    assert agent_receipt['shutdown'] in ['passed', 'clean']
    agents = agent_receipt['selected_agent_instances']
    assert agents and all(all(instance.get(field) for field in
        ['instance', 'package_id', 'package_revision', 'execution_class']) for instance in agents)
    deployment = json.loads((args.human_root / 'operator-input.json').read_text())['deployment']
    assert agent_receipt['target_deployment'] == deployment, 'Agent proof belongs to another deployment'
    fixture = Path(__file__).parent.resolve()
    root = args.directory or Path(tempfile.mkdtemp(prefix='lenso-ops-installations-'))
    root.mkdir(exist_ok=True)
    cases = []
    profiles = [('business_only', False), ('management_no_network', False),
                ('management_mcp_no_console', False), ('management_console_no_agent', True),
                ('management_read_only_no_approval', False)]
    console_case = None
    resumed_profiles = []
    cli_digest = hashlib.sha256(args.cli.read_bytes()).hexdigest()
    for name, human in profiles:
        case = root/name
        if args.resume_known_profiles and case.is_dir():
            facts, source = case/'facts', case/'project'
            facilities = facts/'facilities.json'
            inputs = json.loads((facts/'operator-input.json').read_text())
            prepared = args.human_root if human else args.api_root
            assert inputs == json.loads((prepared/'operator-input.json').read_text())
            completed = case/'qualified-profile.json'
            if completed.exists():
                evidence = json.loads(completed.read_text())
                assert evidence['profile'] == name and evidence['shutdown'] == 'passed'
                assert evidence.get('cli_sha256', cli_digest) == cli_digest
                assert evidence['build']['receipts'] == native_build(case/'distribution')['receipts']
                cases.append(evidence)
                resumed_profiles.append(name)
                if name == 'management_console_no_agent':
                    console_case = (case, facts, source, facilities, inputs, case/'distribution')
                continue
        else:
            case, facts, source, facilities, inputs = prepare_case(fixture, root,
                args.human_root if human else args.api_root, name, human)
        if name == 'business_only':
            shutil.rmtree(source / 'app/management')
            for plugin in (source / 'plugins').iterdir():
                if plugin.is_dir() and plugin.name != 'example.ops-state':
                    shutil.rmtree(plugin)
            template = case / 'template-choices.json'
            original = json.loads(template.read_text()) if template.exists() else {'schema_version': 1, 'choices': []}
            original['choices'] = [choice for choice in original['choices']
                if choice['consumer']['plugin_id'] in ['example.ops-web', 'example.ops-state']
                and (choice.get('provider') is None or choice['provider']['plugin_id'] == 'example.ops-state')]
            (source / 'plugins/.dependencies.json').write_text(json.dumps(original))
            private(facilities, json.dumps({'schema': 'lenso.host-facilities.v1', 'instances': {
                key: value for key, value in json.loads(facilities.read_text())['instances'].items()
                if key.startswith('example.ops-state/')}}))
        elif name == 'management_no_network':
            disable(source, [WEB, MCP, *TOOLS, 'lenso.web-ingress'])
        elif name == 'management_mcp_no_console':
            disable(source, [WEB, *TOOLS, 'lenso.web-ingress'])
        elif name == 'management_console_no_agent':
            disable(source, [*TOOLS, MCP])
        elif name == 'management_read_only_no_approval':
            disable(source, [*TOOLS, MCP, APPROVAL])
            configuration(source, MANAGEMENT, {'deployment': inputs['deployment'],
                'target_instance': 'example.ops-state/primary', 'read_only': True})
            dependencies = source / 'plugins/.dependencies.json'
            choices = json.loads(dependencies.read_text())
            choices['choices'] = [choice for choice in choices['choices'] if not (
                choice['consumer']['plugin_id'] == MANAGEMENT and choice['requirement_id'] == 'approval')]
            choices['choices'].append({'consumer': {'plugin_id': MANAGEMENT, 'instance_key': 'default'},
                'requirement_id': 'approval', 'provider': None})
            choices['choices'].sort(key=lambda choice: (choice['consumer']['plugin_id'],
                choice['consumer']['instance_key'], choice['requirement_id']))
            dependencies.write_text(json.dumps(choices, indent=2) + '\n')
        artifact = case / 'distribution'
        selected, evidence = build(args.cli, source, artifact, facilities)
        with selected_host(args.cli, artifact, facilities, case) as (url, processes):
            process_tree_ids = list(processes)
            checks = {}
            if name == 'business_only':
                assert url
                status, state = request(url, '/state/primary')
                assert status == 200 and {key: state[key] for key in ['value', 'revision']} == api_baseline
                assert request(url, '/management/catalog')[0] == 404
                assert not any('management' in instance or instance.startswith('lenso.auth.') for instance in selected)
                checks['management_and_auth_absent'] = True
                checks['business_preserved'] = True
            elif name == 'management_no_network':
                assert url is None and 'lenso.web-ingress/default' not in selected
                sockets = subprocess.run(['/usr/sbin/lsof', '-Pan', '-p', ','.join(map(str, process_tree_ids)),
                    '-iTCP', '-sTCP:LISTEN'], capture_output=True, text=True)
                assert sockets.returncode in [0, 1] and not sockets.stdout.strip(), 'Network listener in Management-only profile'
                checks['network_surfaces_absent'] = True
            elif name == 'management_mcp_no_console':
                assert url is None and WEB + '/default' not in selected
                prove_mcp(inputs['resource_uri'], (facts / 'tokens/bob.secret').read_text(),
                          revision=api_baseline['revision'])
                checks['actual_mcp_without_console'] = True
            elif name == 'management_read_only_no_approval':
                assert url and APPROVAL + '/default' not in selected
                token = (facts / 'tokens/bob.secret').read_text()
                status, catalog = request(url, '/management/catalog', token)
                assert status == 200 and {entry['id'] for entry in catalog['entries']} == {'state.read'}
                status, read = request(url, '/management/invoke', token, {'entry_id': 'state.read',
                    'version': '2.0.0', 'input_json': '{}', 'idempotency_key': None, 'expected_revision': None})
                assert status == 200 and read['state'] == 'succeeded'
                assert {key: json.loads(read['result_json'])[key] for key in ['value', 'revision']} == api_baseline
                denied_status, _ = request(url, '/management/invoke', token, {'entry_id': 'state.update',
                    'version': '2.0.0', 'input_json': '{"value":62}',
                    'idempotency_key': 'readonly-write-denied', 'expected_revision': str(api_baseline['revision'])})
                assert denied_status == 404, 'The absent write entry must return NotFound, not owner unavailability'
                status, after_denial = request(url, '/management/invoke', token, {'entry_id': 'state.read',
                    'version': '2.0.0', 'input_json': '{}', 'idempotency_key': None, 'expected_revision': None})
                assert status == 200 and after_denial['state'] == 'succeeded'
                assert {key: json.loads(after_denial['result_json'])[key] for key in ['value', 'revision']} == api_baseline
                checks = {'approval_owner_absent': True, 'read_passed': True, 'write_denied': True,
                    'write_rejection_status': denied_status, 'business_preserved': True}
            else:
                assert url
                checks = prove_human(url, facts, baseline)
            assert not any(plugin + '/default' in selected for plugin in BOOTSTRAPS)
        case_evidence = {'profile': name, 'selected_instances': sorted(selected), 'checks': checks,
                         'runtime_process_count': len(processes), 'build': evidence, 'shutdown': 'passed',
                         'cli_sha256':cli_digest}
        cases.append(case_evidence)
        if name == 'management_console_no_agent':
            console_case = (case, facts, source, facilities, inputs, artifact)
        if name == 'management_read_only_no_approval':
            configuration(source, MANAGEMENT, {'deployment': inputs['deployment'],
                'target_instance': 'example.ops-state/primary', 'read_only': False})
            unsafe = case / 'write-without-approval'
            build(args.cli, source, unsafe, facilities)
            reject_write_without_approval(args.cli, unsafe, facilities, case)
            case_evidence['checks']['write_profile_missing_approval'] = 'rejected_before_ready'
        (case/'qualified-profile.json').write_text(json.dumps(case_evidence, indent=2)+'\n')
    cases.append({'profile': 'management_console_with_agent',
        'agent_topology': 'explicit independent Native Model/Loop Host over fixed scoped delegation',
        'selected_agent_instances': agents,
        'checks': {'actual_owner_issued_child': True, 'console_live_during_agent': True,
                   'same_management_process': True, 'business_preserved': True},
        'qualification': {'path': str(args.agent_receipt.resolve()),
            'sha256': hashlib.sha256(args.agent_receipt.read_bytes()).hexdigest()},
        'build': agent_receipt['target_build'], 'shutdown': agent_receipt['shutdown']})
    # Re-open the already built Console-only profile after removing the tools profile.
    case, facts, source, facilities, inputs, artifact = console_case
    with selected_host(args.cli, artifact, facilities, case) as (url, _):
        prove_human(url, facts, baseline)
    # A required security provider cannot be disabled while keeping the selected management entry.
    disable(source, ['lenso.access-control.postgres'])
    rejected = subprocess.run([str(args.cli), 'app', 'build', '--root', str(source),
        '--out', str(case / 'unsafe-distribution')], capture_output=True, text=True)
    rejection = rejected.stdout + rejected.stderr
    assert rejected.returncode != 0 and 'no provider for Capability `lenso.access-control@1`' in rejection, \
        'Removing required Access Control did not produce the expected missing-provider rejection: ' + rejection
    receipt = {'schema': 'lenso.ops-optional-installations.v1', 'target': 'native', 'cases': cases,
        'cli_sha256':cli_digest, 'resumed_completed_profiles':resumed_profiles,
        'agent_removal_preserves_console': 'passed',
        'required_security_removal': 'rejected', 'bootstrap_repeated': False,
        'business_baseline': {'api': api_baseline, 'human': baseline}, 'workers_management': 'not_run',
        'agent_model_loop_profile': 'passed_explicit_independent_native_profile',
        'real_model': 'not_run_user_deferred'}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'receipt': str(args.receipt), 'profiles': [case['profile'] for case in cases]}))


if __name__ == '__main__':
    run()
