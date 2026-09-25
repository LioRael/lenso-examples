"""One persistent, framed ArcBox exec for disposable local KB qualification.

The verified task-owned workerd container shares PostgreSQL's internal network
namespace. No private bridge port is accepted or published.
"""

import argparse
import asyncio
import json
import struct

from verify_workers_settings_tunnel import exact_workerd_relay


HEADER = struct.Struct(">BII")
OPEN, DATA, CLOSE, READY = 1, 2, 3, 4
MAX_FRAME = 65_536
MAX_STREAM_BUFFER = 4 * MAX_FRAME

NODE_MUX = r"""
const net = require('node:net');
const port = Number(process.argv[1]);
const sockets = new Map();
let pending = Buffer.alloc(0);
let outputBlocked = false;

process.stdout.on('drain', () => {
  outputBlocked = false;
  for (const socket of sockets.values()) socket.resume();
});

function frame(op, id, bytes = Buffer.alloc(0)) {
  for (let offset = 0; offset < bytes.length || offset === 0; offset += 65536) {
    const part = bytes.subarray(offset, offset + 65536);
    const header = Buffer.alloc(9);
    header.writeUInt8(op, 0);
    header.writeUInt32BE(id, 1);
    header.writeUInt32BE(part.length, 5);
    if (!process.stdout.write(Buffer.concat([header, part]))) {
      outputBlocked = true;
      for (const socket of sockets.values()) socket.pause();
    }
    if (!bytes.length) break;
  }
}

process.stdin.on('data', chunk => {
  pending = Buffer.concat([pending, chunk]);
  while (pending.length >= 9) {
    const op = pending.readUInt8(0);
    const id = pending.readUInt32BE(1);
    const length = pending.readUInt32BE(5);
    if (length > 65536 || (op !== 1 && op !== 2 && op !== 3)) process.exit(2);
    if (pending.length < 9 + length) break;
    const bytes = pending.subarray(9, 9 + length);
    pending = pending.subarray(9 + length);
    if (op === 1) {
      if (length || sockets.has(id) || sockets.size >= 16) process.exit(2);
      const socket = net.connect({host: '127.0.0.1', port});
      sockets.set(id, socket);
      socket.on('data', data => frame(2, id, data));
      socket.on('connect', () => { if (outputBlocked) socket.pause(); });
      socket.on('drain', () => process.stdin.resume());
      socket.on('error', () => {});
      socket.on('close', () => { sockets.delete(id); frame(3, id); });
    } else if (op === 2) {
      const socket = sockets.get(id);
      if (socket && !socket.write(bytes)) process.stdin.pause();
    } else {
      const socket = sockets.get(id);
      if (socket) socket.end();
    }
  }
});
process.stdin.on('end', () => { for (const socket of sockets.values()) socket.destroy(); });
frame(4, 0);
"""


async def read_frame(reader):
    op, stream_id, length = HEADER.unpack(await reader.readexactly(HEADER.size))
    if length > MAX_FRAME:
        raise ValueError("relay frame exceeds bound")
    return op, stream_id, await reader.readexactly(length)


async def serve(container_id, inside_port):
    process = await asyncio.create_subprocess_exec(
        "docker", "exec", "-i", container_id, "node", "-e", NODE_MUX,
        str(inside_port), stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL,
    )
    sockets = {}
    send_lock = asyncio.Lock()
    slots = asyncio.Semaphore(16)
    next_id = 0

    async def send(op, stream_id, payload=b""):
        if len(payload) > MAX_FRAME:
            raise ValueError("relay frame exceeds bound")
        async with send_lock:
            process.stdin.write(HEADER.pack(op, stream_id, len(payload)) + payload)
            await process.stdin.drain()

    async def from_container():
        while True:
            op, stream_id, payload = await read_frame(process.stdout)
            if stream_id not in sockets:
                continue
            writer, closed = sockets[stream_id]
            if op == DATA:
                if not writer.is_closing():
                    writer.write(payload)
                    if writer.transport.get_write_buffer_size() > MAX_STREAM_BUFFER:
                        closed.set()
                        writer.close()
            elif op == CLOSE and not payload:
                closed.set()
                writer.close()
            else:
                raise ValueError("invalid relay response frame")

    async def relay(reader, writer):
        nonlocal next_id
        try:
            await asyncio.wait_for(slots.acquire(), timeout=2)
        except asyncio.TimeoutError:
            writer.close()
            return
        try:
            next_id += 1
            stream_id = next_id
            closed = asyncio.Event()
            sockets[stream_id] = (writer, closed)
            try:
                await send(OPEN, stream_id)
                while data := await reader.read(MAX_FRAME):
                    await send(DATA, stream_id, data)
                await send(CLOSE, stream_id)
                await asyncio.wait_for(closed.wait(), timeout=10)
            except (BrokenPipeError, ConnectionError, OSError, asyncio.TimeoutError):
                pass
            finally:
                sockets.pop(stream_id, None)
                writer.close()
        finally:
            slots.release()

    server = None
    pump = None
    server_task = None
    try:
        if await asyncio.wait_for(read_frame(process.stdout), timeout=5) != (READY, 0, b""):
            raise RuntimeError("Docker relay did not send its readiness frame")
        pump = asyncio.create_task(from_container())
        server = await asyncio.start_server(relay, "127.0.0.1", 0,
                                            limit=MAX_FRAME, backlog=16)
        port = server.sockets[0].getsockname()[1]
        print(json.dumps({"listen_host": "127.0.0.1", "listen_port": port,
                          "inside_port": inside_port, "container_id": container_id,
                          "relay_kind": "persistent_node_mux"}), flush=True)
        server_task = asyncio.create_task(server.serve_forever())
        done, _ = await asyncio.wait((pump, server_task), return_when=asyncio.FIRST_COMPLETED)
        for task in done:
            task.result()
        raise RuntimeError("Docker relay stopped unexpectedly")
    finally:
        if server is not None:
            server.close()
            await server.wait_closed()
        for task in (pump, server_task):
            if task is not None:
                task.cancel()
        await asyncio.gather(*(task for task in (pump, server_task) if task is not None),
                             return_exceptions=True)
        for writer, closed in sockets.values():
            closed.set()
            writer.close()
        if process.returncode is None:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), timeout=2)
            except asyncio.TimeoutError:
                process.kill()
                await process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", required=True)
    parser.add_argument("--inside-port", type=int, choices=(5432, 8787), required=True)
    args = parser.parse_args()
    container_id = exact_workerd_relay(args.container)
    asyncio.run(serve(container_id, args.inside_port))


if __name__ == "__main__":
    main()
