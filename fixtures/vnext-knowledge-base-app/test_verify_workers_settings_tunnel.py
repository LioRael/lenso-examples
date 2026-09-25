"""Admission checks for the task-owned ArcBox loopback fallback."""

import json
import subprocess
import unittest
from unittest.mock import patch

from verify_workers_settings_tunnel import (
    NODE_IMAGE,
    exact_internal_container,
    exact_postgres_relay,
    exact_workerd_relay,
)


def result(value):
    return subprocess.CompletedProcess([], 0, json.dumps([value]), "")


class WorkersSettingsTunnelTests(unittest.TestCase):
    def test_rejects_other_containers_before_docker_inspection(self):
        with patch("verify_workers_settings_tunnel.subprocess.run") as run:
            for name in ("postgres", "lenso-a6-runtime-krrjje-pg", "lenso-kb-settings-ns-925a;id"):
                with self.assertRaises(ValueError):
                    exact_internal_container(name)
            run.assert_not_called()

    def test_requires_running_container_on_exactly_one_internal_network(self):
        container = {
            "Id": "verified-container-id",
            "State": {"Status": "running"},
            "HostConfig": {"NetworkMode": "lenso-kb-settings-net-925a"},
            "NetworkSettings": {"Networks": {"lenso-kb-settings-net-925a": {
                "NetworkID": "verified-network-id"
            }}},
        }
        with patch("verify_workers_settings_tunnel.subprocess.run",
                   side_effect=[result(container), result({
                       "Internal": True, "Id": "verified-network-id"
                   })]) as run:
            self.assertEqual(exact_internal_container("lenso-kb-settings-ns-925a"),
                             "verified-container-id")
            self.assertEqual(run.call_args_list[1].args[0],
                             ["docker", "network", "inspect", "lenso-kb-settings-net-925a"])
        for altered in (
            {**container, "State": {"Status": "exited"}},
            {**container, "NetworkSettings": {"Networks": {"one": {}, "two": {}}}},
        ):
            with patch("verify_workers_settings_tunnel.subprocess.run", return_value=result(altered)):
                with self.assertRaises(ValueError):
                    exact_internal_container("lenso-kb-settings-ns-925a")
        with patch("verify_workers_settings_tunnel.subprocess.run",
                   side_effect=[result(container), result({
                       "Internal": False, "Id": "verified-network-id"
                   })]):
            with self.assertRaises(ValueError):
                exact_internal_container("lenso-kb-settings-ns-925a")
        with patch("verify_workers_settings_tunnel.subprocess.run",
                   side_effect=[result(container), result({
                       "Internal": True, "Id": "different-network-id"
                   })]):
            with self.assertRaises(ValueError):
                exact_internal_container("lenso-kb-settings-ns-925a")

    def test_postgres_relay_requires_the_exact_task_anchor_network_chain(self):
        anchor = {
            "Id": "verified-anchor-id",
            "State": {"Status": "exited"},
            "HostConfig": {"NetworkMode": "lenso-kb-settings-net-925a"},
            "NetworkSettings": {"Networks": {"lenso-kb-settings-net-925a": {
                "NetworkID": "verified-network-id"
            }}},
        }
        postgres = {
            "Id": "verified-postgres-id",
            "State": {"Status": "running"},
            "HostConfig": {"NetworkMode": "container:verified-anchor-id"},
            "NetworkSettings": {"Networks": {}},
        }
        with patch("verify_workers_settings_tunnel.subprocess.run", side_effect=[
            result(postgres), result(anchor), result({"Internal": True, "Id": "verified-network-id"}),
        ]) as run:
            self.assertEqual(exact_postgres_relay("lenso-kb-settings-pg-925a"),
                             "verified-postgres-id")
            self.assertEqual(run.call_args_list[1].args[0],
                             ["docker", "inspect", "lenso-kb-settings-ns-925a"])
        with patch("verify_workers_settings_tunnel.subprocess.run", side_effect=[
            result({**postgres, "HostConfig": {"NetworkMode": "bridge"}}),
            result(anchor), result({"Internal": True, "Id": "verified-network-id"}),
        ]):
            with self.assertRaises(ValueError):
                exact_postgres_relay("lenso-kb-settings-pg-925a")

    def test_workerd_relay_requires_fixed_image_and_postgres_namespace(self):
        anchor = {
            "Id": "anchor-id", "State": {"Status": "exited"},
            "HostConfig": {"NetworkMode": "lenso-kb-settings-net-925a"},
            "NetworkSettings": {"Networks": {"lenso-kb-settings-net-925a": {
                "NetworkID": "internal-network-id"
            }}},
        }
        postgres = {
            "Id": "postgres-id", "State": {"Status": "running"},
            "HostConfig": {"NetworkMode": "container:anchor-id"},
            "NetworkSettings": {"Networks": {}},
        }
        workerd = {
            "Id": "workerd-id", "State": {"Status": "running"},
            "Config": {"Image": NODE_IMAGE},
            "HostConfig": {"NetworkMode": "container:postgres-id"},
            "NetworkSettings": {"Networks": {}},
        }
        responses = [result(postgres), result(anchor),
                     result({"Internal": True, "Id": "internal-network-id"}), result(workerd)]
        with patch("verify_workers_settings_tunnel.subprocess.run", side_effect=responses):
            self.assertEqual(exact_workerd_relay("lenso-kb-settings-workerd-e3r-925a"), "workerd-id")
        for altered in (
            {**workerd, "Config": {"Image": "node:latest"}},
            {**workerd, "HostConfig": {"NetworkMode": "bridge"}},
            {**workerd, "NetworkSettings": {"Networks": {"other": {}}}},
        ):
            responses[-1] = result(altered)
            with patch("verify_workers_settings_tunnel.subprocess.run", side_effect=responses):
                with self.assertRaises(ValueError):
                    exact_workerd_relay("lenso-kb-settings-workerd-e3r-925a")
        with patch("verify_workers_settings_tunnel.subprocess.run") as run:
            with self.assertRaises(ValueError):
                exact_workerd_relay("unrelated-workerd")
            run.assert_not_called()
        with patch("verify_workers_settings_tunnel.subprocess.run", side_effect=[
            result(postgres), result({**anchor, "State": {"Status": "paused"}}),
        ]):
            with self.assertRaises(ValueError):
                exact_postgres_relay("lenso-kb-settings-pg-925a")


if __name__ == "__main__":
    unittest.main()
