"""Local-only ArcBox fallback for public workerd and disposable PostgreSQL TCP.

The namespace anchor must have exactly one internal Docker network. If ArcBox
cannot exec into the anchor or PostgreSQL container, a task-owned workerd
container may relay only when it shares PostgreSQL's verified namespace and
uses the fixed Node image. This tool never forwards the private settings
bridge, which stays on loopback in that namespace.
"""

import argparse
import asyncio
import json
import re
import subprocess


NODE_IMAGE = "node@sha256:24b8bc17702002d2ed0c1da9ad66c3ee507cc279d0856726662ff2b6fc35c149"


NODE_RELAY = """
const net = require('node:net');
const socket = net.connect({host: '127.0.0.1', port: Number(process.argv[1])});
socket.on('error', () => { process.exitCode = 1; process.stdin.destroy(); });
process.stdin.pipe(socket);
socket.pipe(process.stdout);
socket.on('close', () => process.stdin.destroy());
"""


def exact_internal_container(name, *, allow_exited=False):
    if not re.fullmatch(r"lenso-kb-settings-ns-[a-z0-9]+", name):
        raise ValueError("tunnel is limited to a task-owned KB namespace container")
    result = subprocess.run(
        ["docker", "inspect", name], check=True, capture_output=True, text=True
    )
    container, = json.loads(result.stdout)
    if container["State"]["Status"] != "running" and not (
        allow_exited and container["State"]["Status"] == "exited"
    ):
        raise ValueError("KB namespace container is not running")
    networks = container["NetworkSettings"]["Networks"]
    if len(networks) != 1:
        raise ValueError("KB namespace container must have exactly one network")
    network_name, = networks
    result = subprocess.run(
        ["docker", "network", "inspect", network_name],
        check=True, capture_output=True, text=True,
    )
    network, = json.loads(result.stdout)
    if (not network["Internal"] or networks[network_name]["NetworkID"] != network["Id"]
            or container["HostConfig"]["NetworkMode"] != network_name):
        raise ValueError("KB namespace must be on an internal Docker network")
    return container["Id"]


def exact_postgres_relay(name):
    match = re.fullmatch(r"lenso-kb-settings-pg-([a-z0-9]+)", name)
    if not match:
        raise ValueError("PostgreSQL relay must be a task-owned KB container")
    result = subprocess.run(
        ["docker", "inspect", name], check=True, capture_output=True, text=True
    )
    container, = json.loads(result.stdout)
    if container["State"]["Status"] != "running":
        raise ValueError("KB PostgreSQL relay is not running")
    # The anchor can expire while a still-running PostgreSQL container retains
    # its network namespace. Relay only through that exact container.
    anchor = exact_internal_container("lenso-kb-settings-ns-" + match[1], allow_exited=True)
    if container["HostConfig"]["NetworkMode"] != "container:" + anchor:
        raise ValueError("KB PostgreSQL relay does not share the internal namespace")
    if container["NetworkSettings"]["Networks"]:
        raise ValueError("KB PostgreSQL relay has an additional network")
    return container["Id"]


def exact_workerd_relay(name):
    match = re.fullmatch(r"lenso-kb-settings-workerd-[a-z0-9]+-([a-z0-9]+)", name)
    if not match:
        raise ValueError("Workerd relay must be a task-owned KB container")
    postgres_id = exact_postgres_relay("lenso-kb-settings-pg-" + match[1])
    result = subprocess.run(
        ["docker", "inspect", name], check=True, capture_output=True, text=True
    )
    container, = json.loads(result.stdout)
    if (container["State"]["Status"] != "running"
            or container["Config"]["Image"] != NODE_IMAGE
            or container["HostConfig"]["NetworkMode"] != "container:" + postgres_id
            or container["NetworkSettings"]["Networks"]):
        raise ValueError("Workerd relay does not share the verified internal KB namespace")
    return container["Id"]


async def serve(container_id, inside_port, relay_kind):
    slots = asyncio.Semaphore(4)

    async def relay(reader, writer):
        async with slots:
            process = None
            upstream_task = None
            downstream_task = None
            try:
                command = (
                    ["node", "-e", NODE_RELAY, str(inside_port)]
                    if relay_kind == "node" else
                    ["nc", "-n", "127.0.0.1", str(inside_port)]
                )
                process = await asyncio.create_subprocess_exec(
                    "docker", "exec", "-i", container_id, *command,
                    stdin=asyncio.subprocess.PIPE,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.DEVNULL,
                )

                async def upstream():
                    try:
                        while data := await reader.read(65_536):
                            process.stdin.write(data)
                            await process.stdin.drain()
                    finally:
                        process.stdin.close()

                async def downstream():
                    try:
                        while data := await process.stdout.read(65_536):
                            writer.write(data)
                            await writer.drain()
                    finally:
                        writer.close()

                upstream_task = asyncio.create_task(upstream())
                downstream_task = asyncio.create_task(downstream())
                done, _ = await asyncio.wait(
                    (upstream_task, downstream_task), return_when=asyncio.FIRST_COMPLETED
                )
                for task in done:
                    task.result()
                if upstream_task in done and downstream_task not in done:
                    # Some nc implementations keep stdout open after client EOF.
                    # Allow a final response, then reap the task-owned exec.
                    try:
                        await asyncio.wait_for(downstream_task, timeout=2)
                    except asyncio.TimeoutError:
                        pass
            except (BrokenPipeError, ConnectionError, OSError):
                pass
            finally:
                for task in (upstream_task, downstream_task):
                    if task is not None and not task.done():
                        task.cancel()
                await asyncio.gather(
                    *(task for task in (upstream_task, downstream_task) if task is not None),
                    return_exceptions=True,
                )
                writer.close()
                if process is not None and process.returncode is None:
                    process.terminate()
                    try:
                        await asyncio.wait_for(process.wait(), timeout=2)
                    except asyncio.TimeoutError:
                        process.kill()
                        await process.wait()

    server = await asyncio.start_server(relay, "127.0.0.1", 0, limit=65_536)
    port = server.sockets[0].getsockname()[1]
    print(json.dumps({"listen_host": "127.0.0.1", "listen_port": port,
                      "inside_port": inside_port, "container_id": container_id,
                      "relay_kind": relay_kind}), flush=True)
    async with server:
        await server.serve_forever()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", required=True)
    parser.add_argument("--inside-port", type=int, choices=(5432, 8787), required=True)
    args = parser.parse_args()
    if args.container.startswith("lenso-kb-settings-workerd-"):
        container_id = exact_workerd_relay(args.container)
        relay_kind = "node"
    elif args.container.startswith("lenso-kb-settings-pg-"):
        container_id = exact_postgres_relay(args.container)
        relay_kind = "postgres_nc"
    else:
        container_id = exact_internal_container(args.container)
        relay_kind = "node"
    asyncio.run(serve(container_id, args.inside_port, relay_kind))


if __name__ == "__main__":
    main()
