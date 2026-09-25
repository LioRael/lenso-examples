"""Local-only ArcBox fallback for public workerd and disposable PostgreSQL TCP.

The selected container must have exactly one internal Docker network. This
tool never forwards the private settings bridge, which stays on loopback in
the workerd network namespace. It is needed only when ArcBox's ordinary
127.0.0.1 port publisher accepts connections but fails to pass their bytes.
"""

import argparse
import asyncio
import json
import re
import subprocess


NODE_RELAY = """
const net = require('node:net');
const socket = net.connect({host: '127.0.0.1', port: Number(process.argv[1])});
socket.on('error', () => { process.exitCode = 1; process.stdin.destroy(); });
process.stdin.pipe(socket);
socket.pipe(process.stdout);
socket.on('close', () => process.stdin.destroy());
"""


def exact_internal_container(name):
    if not re.fullmatch(r"lenso-kb-settings-ns-[a-z0-9]+", name):
        raise ValueError("tunnel is limited to a task-owned KB namespace container")
    result = subprocess.run(
        ["docker", "inspect", name], check=True, capture_output=True, text=True
    )
    container, = json.loads(result.stdout)
    if container["State"]["Status"] != "running":
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
    if not network["Internal"]:
        raise ValueError("KB namespace must be on an internal Docker network")
    return container["Id"]


async def serve(container_id, inside_port):
    slots = asyncio.Semaphore(16)

    async def relay(reader, writer):
        async with slots:
            process = None
            try:
                process = await asyncio.create_subprocess_exec(
                    "docker", "exec", "-i", container_id, "node", "-e", NODE_RELAY,
                    str(inside_port), stdin=asyncio.subprocess.PIPE,
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

                await asyncio.gather(upstream(), downstream())
                await process.wait()
            except (BrokenPipeError, ConnectionError, OSError):
                pass
            finally:
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
                      "inside_port": inside_port, "container_id": container_id}), flush=True)
    async with server:
        await server.serve_forever()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", required=True)
    parser.add_argument("--inside-port", type=int, choices=(5432, 8787), required=True)
    args = parser.parse_args()
    container_id = exact_internal_container(args.container)
    asyncio.run(serve(container_id, args.inside_port))


if __name__ == "__main__":
    main()
