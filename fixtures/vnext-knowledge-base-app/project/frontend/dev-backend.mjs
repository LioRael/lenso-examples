import { closeSync, constants, fstatSync, openSync, readSync } from 'node:fs';
import { request as httpRequest } from 'node:http';

const publicRoutes = [
  [/^\/notes$/, ['POST']],
  [/^\/notes\/[^/]+$/, ['GET']],
  [/^\/jobs\/process-next$/, ['POST']],
  [/^\/job-status\/[^/]+$/, ['GET']],
  [/^\/settings$/, ['GET', 'PUT']],
  [/^\/note-attachments\/[^/]+$/, ['POST']],
];
const hopHeaders = ['connection', 'proxy-connection', 'keep-alive', 'upgrade',
  'te', 'trailer', 'transfer-encoding', 'proxy-authenticate', 'proxy-authorization'];

function currentBackend(urlFile) {
  const descriptor = openSync(urlFile, constants.O_RDONLY
    | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
  let file;
  try {
    const initial = fstatSync(descriptor);
    if (!initial.isFile() || initial.nlink !== 1 || initial.size < 1 || initial.size > 512) {
      throw new Error('the current backend URL file is not a bounded regular file');
    }
    const bytes = Buffer.alloc(initial.size + 1);
    let count = 0;
    while (count < bytes.length) {
      const read = readSync(descriptor, bytes, count, bytes.length - count, count);
      if (read === 0) break;
      count += read;
    }
    const final = fstatSync(descriptor);
    if (count !== initial.size || final.size !== initial.size
      || final.dev !== initial.dev || final.ino !== initial.ino) {
      throw new Error('the current backend URL file changed while reading');
    }
    file = bytes.subarray(0, count).toString('utf8');
  } finally {
    closeSync(descriptor);
  }
  const address = file.endsWith('\n') ? file.slice(0, -1).replace(/\r$/, '') : file;
  if (!/^http:\/\/(?:127\.0\.0\.1|\[::1\]):[1-9][0-9]{0,4}\/?$/.test(address)) {
    throw new Error('the current backend URL is not a loopback HTTP origin');
  }
  const url = new URL(address);
  if (Number(url.port) > 65535) throw new Error('the current backend port is invalid');
  return { address, url };
}

function unavailable(response) {
  response.writeHead(503, {
    'cache-control': 'no-store',
    'content-type': 'text/plain; charset=utf-8',
  });
  response.end('Lenso development backend is unavailable');
}

export function createDevBackendMiddleware(urlFile) {
  return (request, response, next) => {
    const incoming = new URL(request.url ?? '/', 'http://lenso.local');
    const pathname = incoming.pathname;
    const handshake = pathname === '/__lenso/backend' && request.method === 'GET';
    if (!handshake && !publicRoutes.some(([path, methods]) =>
      path.test(pathname) && methods.includes(request.method))) {
      next();
      return;
    }

    let backend;
    try {
      backend = currentBackend(urlFile);
    } catch {
      unavailable(response);
      return;
    }

    if (handshake) {
      response.writeHead(200, {
        'cache-control': 'no-store',
        'content-length': Buffer.byteLength(backend.address),
        'content-type': 'text/plain; charset=utf-8',
      });
      response.end(backend.address);
      return;
    }

    const headers = { ...request.headers, host: backend.url.host };
    for (const name of hopHeaders) {
      delete headers[name];
    }
    const target = httpRequest({
      hostname: backend.url.hostname.replace(/^\[|\]$/g, ''),
      port: backend.url.port,
      path: incoming.pathname + incoming.search,
      method: request.method,
      headers,
    }, (upstream) => {
      const responseHeaders = { ...upstream.headers };
      for (const name of hopHeaders) {
        delete responseHeaders[name];
      }
      response.writeHead(upstream.statusCode ?? 502, responseHeaders);
      upstream.pipe(response);
    });
    target.on('error', () => {
      if (!response.headersSent) unavailable(response);
      else response.destroy();
    });
    request.on('aborted', () => target.destroy());
    request.pipe(target);
  };
}
