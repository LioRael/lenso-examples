"""Inspect existing Management events through the Audit Owner's typed read port."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reader', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True,
                        help='Retained private ordinary-App test directory')
    parser.add_argument('--qualification', type=Path, required=True,
                        help='Completed ordinary-App receipt to supplement')
    parser.add_argument('--expected', type=Path,
                        help='Private expectations including exact operation/response, if retained')
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    root = args.directory.resolve(strict=True)
    reader = args.reader.resolve(strict=True)
    if args.receipt.exists():
        raise ValueError('A supplemental proof must use a new receipt path')
    qualification = json.loads(args.qualification.read_text())
    owner_input = json.loads((root / 'operator-input.json').read_text())
    if (qualification.get('real_database') is not True
            or qualification.get('target') != 'native_http'
            or qualification.get('shutdown') != 'passed'):
        raise ValueError('A completed real-database qualification is required')
    profile = qualification['profile']
    cases = set(qualification['cases'])
    if profile == 'operators-api-token':
        original = {'distinct_human_approval', 'single_business_receipt'} <= cases
        prior = qualification.get('prior_completed_phase', {})
        resumed = (
            {'reconciled_terminal_intent', 'same_intent_has_single_business_receipt'} <= cases
            and isinstance(prior.get('operation_id'), str)
            and bool(prior['operation_id'])
            and prior.get('reconciled_through_current_owner') is True
            and isinstance(prior.get('value'), int)
            and isinstance(prior.get('revision'), int)
        )
        if not (original or resumed):
            raise ValueError('Missing ordinary management proof')
        requester, decider = 'alice', 'bob'
    elif profile == 'password-account-personal-token':
        original = {'distinct_account_approval', 'single_business_commit'} <= cases
        prior = qualification.get('prior_completed_phase', {})
        resumed = (
            {'actual_password_login', 'distinct_live_account_subjects',
             'reconciled_terminal_human_intent', 'single_existing_business_receipt'} <= cases
            and isinstance(prior.get('operation_id'), str)
            and bool(prior['operation_id'])
            and prior.get('business_write_repeated') is False
            and prior.get('enrollment_repeated') is False
        )
        if not (original or resumed):
            raise ValueError('Missing ordinary human proof')
        accounts = json.loads((root / 'human-enrollment.json').read_text())['accounts']
        requester, decider = accounts['alice']['subject'], accounts['bob']['subject']
    else:
        raise ValueError('Unsupported qualification profile')
    deployment = owner_input['deployment']
    if qualification.get('deployment', deployment) != deployment:
        raise ValueError('Deployment mismatch')
    build_hash = qualification['build']['receipts']['.lenso/host-build.json']['sha256']
    if digest(root / 'distribution/.lenso/host-build.json') != build_hash:
        raise ValueError('The retained distribution differs from the qualification')
    if args.expected:
        expected = json.loads(args.expected.read_text())
        if expected['requester_subject'] != requester or expected['decider_subject'] != decider:
            raise ValueError('Expected actors differ from actual enrollment')
        final = qualification['final_business_state']
        prior = qualification.get('prior_completed_phase', {})
        final_state = (expected['value'], expected['revision']) == (final['value'], final['revision'])
        prior_state = (
            expected['operation_id'] == prior.get('operation_id')
            and (prior.get('reconciled_through_current_owner') is True
                 or profile == 'password-account-personal-token'
                 and prior.get('business_write_repeated') is False
                 and prior.get('enrollment_repeated') is False)
            and (expected['value'], expected['revision']) == (prior.get('value'), prior.get('revision'))
        )
        if not (final_state or prior_state):
            raise ValueError('Expected business state is not in the ordinary qualification')
    else:
        final = qualification['final_business_state']
        expected = {'operation_id': None, 'intent_digest': None, 'receipt_id': None,
                    'requester_subject': requester, 'decider_subject': decider,
                    'value': final['value'], 'revision': final['revision']}
    with tempfile.TemporaryDirectory(prefix='audit-expectations-', dir=root) as temporary:
        expected_file = Path(temporary) / 'expected.json'
        expected_file.write_text(json.dumps(expected))
        expected_file.chmod(0o600)
        reader_hash = digest(reader)
        result = subprocess.run([str(reader), str(root / 'operator-input.json'), str(expected_file)],
                                capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise ValueError('The supplemental Owner reader failed')
    proof = json.loads(result.stdout)
    allowed = {'schema', 'status', 'proof_layer', 'deployment', 'operation_id', 'source_instance',
               'event_count', 'actions', 'lookup', 'requester_and_decider_verified', 'actors_distinct',
               'additional_requester_attempts_verified',
               'intent_digest_consistent', 'domain_value_revision_verified',
               'exact_response_receipt_verified', 'metadata_and_actor_identifiers_redacted',
               'append_invoked', 'audit_owner_source', 'shutdown',
               'receipt_matched_actual_business_owner', 'business_owner_api',
               'business_owner_source_sha256', 'inspection_runtime', 'real_database'}
    if (proof.get('status') != 'passed'
            or proof.get('proof_layer') != 'supplemental_owner_kernel_reader'
            or proof.get('deployment') != deployment or proof.get('shutdown') != 'clean'
            or proof.get('append_invoked') is not False or set(proof) != allowed
            or proof.get('domain_value_revision_verified') is not True
            or proof.get('receipt_matched_actual_business_owner') is not True
            or digest(reader) != reader_hash):
        raise ValueError('Unexpected supplemental proof')
    proof['ordinary_qualification'] = {
        'sha256': digest(args.qualification), 'schema': qualification['schema'],
        'profile': profile, 'host_build_sha256': build_hash,
        'cli_sha256': qualification['cli_sha256'],
    }
    proof['reader_executable_sha256'] = reader_hash
    proof['inspection_boundary'] = (
        'A separate fixed, deployment-scoped Kernel reader queried existing Audit Owner facts. '
        'It correlated the opaque audit receipt with the business Owner receipt API. '
        'It did not instrument the original ordinary App graph, initialize storage, or append events.'
    )
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps({'receipt': str(args.receipt), 'status': 'passed',
                      'proof_layer': proof['proof_layer'], 'event_count': proof['event_count']}))


if __name__ == '__main__':
    try:
        run()
    except Exception:
        raise SystemExit('Supplemental Audit verification failed; private inputs were not printed') from None
