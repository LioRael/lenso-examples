"""Read the current Wrangler account and retain account details privately."""
import argparse
import json
from pathlib import Path
import subprocess

from remote_infrastructure import private, record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wrangler', type=Path, required=True)
    parser.add_argument('--prior-account', type=Path, required=True)
    parser.add_argument('--directory', type=Path, required=True)
    options = parser.parse_args()
    options.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    assert not (options.directory / 'whoami.json').exists()
    result = subprocess.run(['node', str(options.wrangler), 'whoami', '--json'],
                            text=True, capture_output=True, check=False)
    record(options.directory / 'whoami-root.command.json',
           {'returncode': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr})
    if result.returncode:
        raise RuntimeError('Read-only Wrangler auth failed; private diagnostic retained')
    observed = json.loads(result.stdout)
    expected = private(options.prior_account)
    assert observed['loggedIn'] is True
    assert any(account['id'] == expected['account_id'] for account in observed['accounts'])
    record(options.directory / 'whoami.json', observed)
    print(json.dumps({'current_account_matches_prior_tested_account': True,
                      'external_mutations': False, 'account_values_emitted': False}))


if __name__ == '__main__':
    main()
