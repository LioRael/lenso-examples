import { closeSync, constants, fstatSync, openSync, readFileSync, readSync } from 'node:fs';
import { request as httpRequest } from 'node:http';

const allowedPublicOperations = new Map([
  ['POST /notes', 'knowledge-base.notes.create'],
  ['GET /notes/{note_id}', 'knowledge-base.notes.read'],
  ['GET /job-status/{job_id}', 'knowledge-base.jobs.inspect'],
  ['GET /settings', 'knowledge-base.settings.read'],
  ['PUT /settings', 'knowledge-base.settings.update'],
  ['POST /note-attachments/{note_id}', 'knowledge-base.attachments.upload'],
]);

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function compilePath(template) {
  if (!template.startsWith('/') || template === '/' || template.endsWith('/')) {
    throw new Error(`invalid public API path template: ${template}`);
  }
  const parameters = [];
  const segments = template.slice(1).split('/').map((segment) => {
    const parameter = /^\{([a-z][a-z0-9_]*)\}$/.exec(segment);
    if (parameter) {
      if (parameters.includes(parameter[1])) throw new Error(`duplicate path parameter: ${template}`);
      parameters.push(parameter[1]);
      return '[^/]+';
    }
    if (!/^[a-z][a-z0-9-]*$/.test(segment)) {
      throw new Error(`invalid public API path template: ${template}`);
    }
    return segment;
  });
  return { pattern: new RegExp(`^/${segments.join('/')}$`), parameters };
}

export function publicRoutesFromOpenApi(document) {
  if (document?.openapi !== '3.1.0' || !isRecord(document.paths)) {
    throw new Error('the public API document must be OpenAPI 3.1 with paths');
  }
  const bearer = document.components?.securitySchemes?.bearerAuth;
  if (!isRecord(bearer) || bearer.type !== 'http' || bearer.scheme !== 'bearer') {
    throw new Error('the public API document must define bearer authentication');
  }
  const routes = [];
  const observed = new Set();
  for (const [path, item] of Object.entries(document.paths)) {
    if (!isRecord(item) || Object.keys(item).length === 0) {
      throw new Error(`invalid public API path item: ${path}`);
    }
    const { pattern, parameters } = compilePath(path);
    const methods = [];
    for (const [method, operation] of Object.entries(item)) {
      const key = `${method.toUpperCase()} ${path}`;
      if (!['get', 'post', 'put'].includes(method) || !isRecord(operation)
        || allowedPublicOperations.get(key) !== operation.operationId || observed.has(key)) {
        throw new Error(`unapproved public API operation: ${key}`);
      }
      const security = operation.security;
      if (!Array.isArray(security) || security.length !== 1 || !isRecord(security[0])
        || Object.keys(security[0]).length !== 1 || !Array.isArray(security[0].bearerAuth)
        || security[0].bearerAuth.length !== 0) {
        throw new Error(`public API operation must require bearer auth: ${key}`);
      }
      const declared = operation.parameters ?? [];
      if (!Array.isArray(declared) || declared.some((value) => !isRecord(value))) {
        throw new Error(`invalid public API parameters: ${key}`);
      }
      const declaredPath = declared.filter((value) => value.in === 'path');
      if (declaredPath.length !== parameters.length
        || new Set(declaredPath.map((value) => value.name)).size !== parameters.length
        || declaredPath.some((value) =>
          value.required !== true || value.schema?.type !== 'string'
          || !parameters.includes(value.name))) {
        throw new Error(`public API path parameters do not match: ${key}`);
      }
      observed.add(key);
      methods.push(method.toUpperCase());
    }
    routes.push([pattern, methods]);
  }
  if (observed.size !== allowedPublicOperations.size) {
    throw new Error('the public API document omits an approved operation');
  }
  return routes;
}

const publicRoutes = publicRoutesFromOpenApi(JSON.parse(
  readFileSync(new URL('./openapi.json', import.meta.url), 'utf8'),
));
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
