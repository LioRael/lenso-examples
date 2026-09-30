#!/usr/bin/env python3
"""Prepare a new private operator profile; never dispatch or deploy an action."""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
from urllib.parse import urlparse
from uuid import UUID

from client import private, write_new

BINDINGS = {"AUTH_DB", "ACCESS_CONTROL_DB", "AUDIT_DB", "APPROVAL_DB", "MANAGEMENT_DB", "OPS_DB"}
LABEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}\Z")
RUNTIME_ALIASES = {"@lenso/workers-runtime/clock": "./owner-assets/workers-runtime/clock.mjs"}


def new_directory(path):
    assert path.is_absolute() and not path.is_symlink()
    path.mkdir(mode=0o700)


def replace(value, replacements):
    if isinstance(value, dict):
        return {key: replace(item, replacements) for key, item in value.items()}
    if isinstance(value, list):
        return [replace(item, replacements) for item in value]
    if isinstance(value, str):
        return replacements.get(value, value)
    return value


def https_origin(value):
    parsed = urlparse(value)
    assert parsed.scheme == "https" and parsed.hostname and parsed.hostname.endswith(".workers.dev")
    assert not parsed.username and not parsed.password and not parsed.query and not parsed.fragment
    assert parsed.path in {"", "/"} and parsed.port in {None, 443}
    return value.rstrip("/")


def prepare(root, template, facts_file, runtime, source_runtime):
    facts = json.loads(private(facts_file))
    assert LABEL.fullmatch(facts["deployment"]) and LABEL.fullmatch(facts["issuer"])
    assert re.fullmatch(r"[a-f0-9]{32}", facts["account_id"])
    facts["graph_origin"] = https_origin(facts["graph_origin"])
    facts["operator_url"] = https_origin(facts["operator_url"])
    mcp_resource = facts["graph_origin"] + "/mcp"
    assert facts.get("mcp_resource", mcp_resource) == mcp_resource
    facts["mcp_resource"] = mcp_resource
    assert urlparse(facts["graph_origin"]).hostname != urlparse(facts["operator_url"]).hostname
    assert set(facts["d1_bindings"]) == BINDINGS
    for binding in facts["d1_bindings"].values():
        assert set(binding) == {"database_id", "database_name"}
        UUID(binding["database_id"])
        assert LABEL.fullmatch(binding["database_name"])
    assert hashlib.sha256((source_runtime / "pkg/lenso_ops_workers_owner_operator_bg.wasm").read_bytes()).hexdigest() == \
        "b840e9c7ee46865d33388cc427e3ac8cac884b33d5f5241de114898a12be2553"
    old = json.loads(private(template / "deployment-facts.json"))
    replacements = {old["deployment"]: facts["deployment"], old["issuer"]: facts["issuer"],
                    old["public_origin"]: facts["graph_origin"]}
    new_directory(root)
    new_directory(runtime)
    write_new(root / "deployment-facts.json", facts)
    for name in ["signing", "pepper", "operator-capability"]:
        write_new(root / (name + ".secret"), secrets.token_urlsafe(48) + "\n")
    configuration = replace(json.loads(private(template / "auth-config.json")), replacements)
    secret_map = {configuration["assertion_signing_key_secret"]: private(root / "signing.secret").strip(),
                  configuration["token_pepper_secret"]: private(root / "pepper.secret").strip()}
    assert len(secret_map) == 2
    write_new(root / "auth-config.provisional.json", configuration)
    write_new(root / "auth-secrets.json", secret_map)
    write_new(root / "operator-profile.json", {"url": facts["operator_url"] + "/_qualification/owners",
              "capability_file": "operator-capability.secret"})
    write_new(root / "auth-public-key-derive.action.json", {"action_id": "auth-public-key-derive",
              "owner": "auth", "operation": "public_key"})
    write_new(root / "cloudflare-secrets-upload.private.json", {
        "OPS_TEST_OPERATOR_CAPABILITY": private(root / "operator-capability.secret").strip(),
        "OPS_AUTH_SECRETS_JSON": json.dumps(secret_map, separators=(",", ":"))})
    for name in ["pkg", "owner-assets"]:
        shutil.copytree(source_runtime / name, runtime / name)
    shutil.copy2(source_runtime / "worker.mjs", runtime / "worker.mjs")
    worker_name = urlparse(facts["operator_url"]).hostname.split(".", 1)[0]
    write_new(runtime / "wrangler.jsonc", {"name": worker_name, "account_id": facts["account_id"],
        "main": "worker.mjs", "compatibility_date": "2026-09-26",
        "alias": RUNTIME_ALIASES,
        "vars": {"OPS_AUTH_CONFIGURATION_JSON": json.dumps(configuration, separators=(",", ":"))},
        "d1_databases": [{"binding": key, **facts["d1_bindings"][key]}
                         for key in sorted(BINDINGS - {"OPS_DB"})]})
    write_new(root / "preparation.json", {"schema": "lenso.ops-private-worker-preparation.v1",
        "state": "public_key_pending", "actions_dispatched": False, "resources_created": False,
        "deployment": facts["deployment"], "secret_values_emitted": False})


def promote(root, template, runtime):
    facts = json.loads(private(root / "deployment-facts.json"))
    observed = json.loads(private(root / "auth-public-key-derive.read.json"))
    assert observed["state"] == "completed" and observed["operation"] == "public_key"
    key = observed["result"]["assertion_public_key"]
    assert isinstance(key, str) and 32 <= len(key) <= 256
    configuration = json.loads(private(root / "auth-config.provisional.json"))
    old = json.loads(private(template / "deployment-facts.json"))
    old_config = json.loads(private(template / "auth-config.json"))
    replacements = {old["deployment"]: facts["deployment"], old["issuer"]: facts["issuer"],
                    old["public_origin"]: facts["graph_origin"], old_config["assertion_public_key"]: key}
    configuration["assertion_public_key"] = key
    configurations = replace(json.loads(private(template / "configurations.json")), replacements)
    configurations["lenso.auth.api-token"] = configuration
    write_new(root / "auth-config.json", configuration)
    write_new(root / "configurations.json", configurations)
    write_new(root / "facilities.json", replace(json.loads(private(template / "facilities.json")), replacements))
    write_new(root / "public-key-promoted.json", {"public_key_source": "actual_owner_wasm_helper",
        "actions_dispatched_by_preparation": False, "redeploy_and_verify_key_before_mutation": True})
    provisional = runtime / "wrangler.bundle-fixed.jsonc"
    if not provisional.exists():
        provisional = runtime / "wrangler.jsonc"
    config = json.loads(private(provisional))
    assert config["alias"] == RUNTIME_ALIASES
    config["vars"]["OPS_AUTH_CONFIGURATION_JSON"] = json.dumps(configuration, separators=(",", ":"))
    # Preserve the deployed provisional file; this reviewed successor is selected explicitly.
    write_new(runtime / "wrangler.public-key.jsonc", config)
    write_new(root / "auth-public-key-confirm.action.json", {"action_id": "auth-public-key-confirm",
              "owner": "auth", "operation": "public_key"})
    write_new(root / "configuration-ready.json", {"schema": "lenso.ops-private-worker-config.v1",
        "state": "configuration_prepared", "deployment": facts["deployment"],
        "configurations": str(root / "configurations.json"), "facilities": str(root / "facilities.json"),
        "requires_key_confirmation": True, "owners_provisioned": False})


def bundle_fix(runtime):
    assert (runtime / "owner-assets/workers-runtime/clock.mjs").is_file()
    config = json.loads(private(runtime / "wrangler.jsonc"))
    assert "alias" not in config
    config["alias"] = RUNTIME_ALIASES
    write_new(runtime / "wrangler.bundle-fixed.jsonc", config)


def actions(root, template):
    confirmed = json.loads(private(root / "auth-public-key-confirm.read.json"))
    config = json.loads(private(root / "auth-config.json"))
    assert confirmed["state"] == "completed" and confirmed["result"]["configured_key_matches"] is True
    assert confirmed["result"]["assertion_public_key"] == config["assertion_public_key"]
    facts = json.loads(private(root / "deployment-facts.json"))
    old = json.loads(private(template / "deployment-facts.json"))
    replacement = {old["deployment"]: facts["deployment"]}
    expires = (datetime.now(timezone.utc) + timedelta(hours=4)).isoformat().replace("+00:00", "Z")
    for owner in ["auth", "access", "audit", "approval", "management"]:
        for operation in ["setup", "verify"]:
            action_id = owner + "-" + operation + "-1"
            write_new(root / (action_id + ".action.json"), {"action_id": action_id,
                      "owner": owner, "operation": operation})
    for name in ["bootstrap", "alice", "bob", "machine", "wrong-deployment"]:
        action = replace(json.loads(private(template / ("issue-" + name + "-1.json"))), replacement)
        action["parameters"]["expires_at"] = expires
        if name == "alice":
            action["parameters"]["audience"] = list(dict.fromkeys(
                [*action["parameters"]["audience"], facts["mcp_resource"]]))
        if name == "wrong-deployment": action["parameters"]["deployment"] = facts["deployment"] + "-other"
        write_new(root / (action["action_id"] + ".action.json"), action)
    bootstrap = json.loads(private(template / "access-bootstrap-after-confirmed-absence-1.json"))
    bootstrap["action_id"] = "access-bootstrap-1"
    write_new(root / "access-bootstrap-1.action.json", bootstrap)
    for subject in ["alice", "bob"]:
        action_id = "qualify-" + subject + "-1"
        write_new(root / (action_id + ".action.json"), {"action_id": action_id,
            "owner": "management", "operation": "grant",
            "parameters": {"deployment": facts["deployment"], "subject": subject}})
    write_new(root / "actions-prepared.json", {"state": "not_dispatched", "expires_at": expires,
        "order": "five setup/verify pairs; bootstrap/alice/bob/machine/wrong-deployment issue+metadata; typed ACL bootstrap+roles read; qualify Alice/Bob",
        "unknown_policy": "Stop on a started phase without a completed receipt; query exact Owner facts, never blind-reissue."})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["prepare", "promote", "actions", "bundle-fix"])
    for flag in ["private-root", "template-private-root", "runtime"]:
        parser.add_argument("--" + flag, required=True, type=Path)
    parser.add_argument("--facts", type=Path)
    parser.add_argument("--source-runtime", type=Path)
    args = parser.parse_args()
    if args.mode == "prepare":
        assert args.facts and args.source_runtime
        prepare(args.private_root, args.template_private_root, args.facts, args.runtime, args.source_runtime)
    elif args.mode == "promote": promote(args.private_root, args.template_private_root, args.runtime)
    elif args.mode == "bundle-fix": bundle_fix(args.runtime)
    else: actions(args.private_root, args.template_private_root)
    print(json.dumps({"state": "prepared_without_dispatch", "mode": args.mode, "secret_values_emitted": False}))


if __name__ == "__main__":
    main()
