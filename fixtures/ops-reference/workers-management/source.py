"""Assemble the ordinary Worker App from its tracked source owners."""

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess


def stage(directory):
    fixture = Path(__file__).resolve().parent
    source = directory / 'project'
    if source.exists():
        raise ValueError('Choose a new source directory; existing candidates are preserved')
    shutil.copytree(fixture.parent / 'project', source,
                    ignore=shutil.ignore_patterns('.lenso', 'target', 'dist'))
    shutil.rmtree(source / 'app/management')
    shutil.copytree(fixture / 'project/app/management', source / 'app/management',
                    ignore=shutil.ignore_patterns('target'))
    shutil.copytree(fixture.parent / 'storage', directory / 'storage',
                    ignore=shutil.ignore_patterns('target'))
    plugins = source / 'plugins'
    for plugin in plugins.iterdir():
        if plugin.is_dir() and plugin.name != 'example.ops-state':
            shutil.rmtree(plugin)
    (plugins / 'example.ops-state/secondary.toml').unlink()
    primary = plugins / 'example.ops-state/primary.toml'
    primary.write_text(primary.read_text().replace('simulated', 'workers-d1'))
    plain_web = plugins / 'example.ops-web'
    plain_web.mkdir()
    (plain_web / 'default.disabled').write_text('')
    return source


def configure(directory, configurations, facilities, *, read_only=False):
    fixture = Path(__file__).resolve().parent
    source = directory / 'project'
    if not source.is_dir():
        raise ValueError('Stage the source tree before configuring its selected owners')
    plugins = source / 'plugins'
    # Use the same bounded TOML serialization as the Native reference assembly.
    import sys
    sys.path.insert(0, str(fixture.parent))
    from verify_management import configuration
    required = {'example.ops-management', 'example.ops-management-web',
                'example.ops-secrets', 'lenso.auth.api-token',
                'lenso.access-control.d1', 'lenso.audit-log.d1'}
    if not read_only:
        required.add('lenso.business-approval.d1')
    if set(configurations) != required:
        raise ValueError('The selected Worker profile must name exactly its expected owners')
    if configurations['example.ops-management']['read_only'] is not read_only:
        raise ValueError('The selected core and approval dependency must agree')
    for plugin, values in configurations.items():
        if plugin == 'example.ops-management-web':
            values = {**values, 'mcp_authentication': values.get('mcp_authentication', 'oauth')}
        configuration(source, plugin, values)
    if read_only:
        approval = plugins / 'lenso.business-approval.d1'
        if approval.exists():
            shutil.rmtree(approval)

    choices = []
    def bind(consumer, dependency, provider):
        choices.append({'consumer': {'plugin_id': consumer, 'instance_key': 'default'},
                        'requirement_id': dependency,
                        'provider': None if provider is None else
                            {'plugin_id': provider, 'instance_key': 'primary'
                                if provider == 'example.ops-state' else 'default'}})
    core = 'example.ops-management'
    for requirement, provider in [('state', 'example.ops-state'),
        ('credential_state', 'lenso.auth.api-token'), ('access', 'lenso.access-control.d1'),
        ('approval', None if read_only else 'lenso.business-approval.d1'),
        ('audit', 'lenso.audit-log.d1')]:
        bind(core, requirement, provider)
    for requirement, provider in [('auth', 'lenso.auth.api-token'),
        ('management', core), ('human', core)]:
        bind('example.ops-management-web', requirement, provider)
    choices.sort(key=lambda item: (item['consumer']['plugin_id'], item['requirement_id']))
    (plugins / '.dependencies.json').write_text(json.dumps(
        {'schema_version': 1, 'choices': choices}, indent=2) + '\n')
    grant = directory / 'facilities.json'
    grant.write_text(json.dumps(facilities, indent=2) + '\n')
    return source, grant


def assemble(directory, configurations, facilities, *, read_only=False):
    stage(directory)
    return configure(directory, configurations, facilities, read_only=read_only)


def resolve(source):
    for directory in [source, source / 'app/state', source / 'app/management',
                      source / 'contracts/example.ops-state',
                      source / 'contracts/example.ops-state-v2']:
        subprocess.run(['cargo', 'metadata', '--format-version', '1'], cwd=directory,
                       stdout=subprocess.DEVNULL, check=True)


def build(cli, source, grant, runtime, wasm_bindgen, limits, output):
    subprocess.run([str(cli), 'app', 'build', '--root', str(source), '--target', 'workers',
                    '--wasm-bindgen', str(wasm_bindgen), '--workers-runtime', str(runtime),
                    '--workers-facilities', str(grant), '--workers-host-limits', str(limits),
                    '--out', str(output)], check=True)


def configure_local_runtime(output, facts_path, secrets_path):
    """Attach the explicit local bindings without starting or initializing Owners."""
    facts = json.loads(facts_path.read_text())
    bindings = facts['d1_bindings']
    expected = {'AUTH_DB', 'ACCESS_CONTROL_DB', 'AUDIT_DB', 'APPROVAL_DB',
                'MANAGEMENT_DB', 'OPS_DB'}
    if {entry['binding'] for entry in bindings} != expected or len(bindings) != len(expected):
        raise ValueError('The local graph requires exactly its six prepared D1 bindings')
    if any(entry.get('remote') is not False for entry in bindings):
        raise ValueError('This helper only configures local D1 bindings')
    metadata = secrets_path.stat()
    if secrets_path.is_symlink() or not stat.S_ISREG(metadata.st_mode) or \
        metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) != 0o600:
        raise ValueError('The Owner secret input must be a private regular file')
    secret_values = json.loads(secrets_path.read_text())
    if set(secret_values) != {'operators/api-signing', 'operators/api-pepper'} or \
        any(not isinstance(value, str) or not 32 <= len(value) <= 16384
            for value in secret_values.values()):
        raise ValueError('The Owner secret map has an unsupported shape')
    encoded = json.dumps(secret_values, separators=(',', ':'))
    if "'" in encoded or '\n' in encoded or '\r' in encoded:
        raise ValueError('The Owner secret map cannot be represented by this private dotenv binding')
    variables = output / '.dev.vars'
    payload = "OPS_AUTH_SECRETS_JSON='" + encoded + "'\n"
    descriptor = os.open(variables, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as destination:
        destination.write(payload)
        destination.flush()
        os.fsync(destination.fileno())
    configuration_path = output / 'wrangler.jsonc'
    configuration = json.loads(configuration_path.read_text())
    configuration.update(name='ops-management-local-' + facts['nonce'], workers_dev=False,
                         observability={'enabled': False}, d1_databases=bindings)
    configuration_path.write_text(json.dumps(configuration, indent=2) + '\n')
    return {'artifact': str(output), 'bindings': sorted(expected),
            'secrets': 'private-dotenv', 'owner_setup': 'not_performed',
            'runtime_start': 'not_performed'}
