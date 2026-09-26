import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { once } from 'node:events';
import test from 'node:test';

import { createDevBackendMiddleware, publicRoutesFromOpenApi } from './dev-backend.mjs';

const publicApi = JSON.parse(await readFile(new URL('./openapi.json', import.meta.url), 'utf8'));

test('the editable OpenAPI document cannot expand the dev proxy boundary', () => {
  assert.equal(publicRoutesFromOpenApi(publicApi).length, 6);

  const extraPath = structuredClone(publicApi);
  extraPath.paths['/admin'] = { get: { operationId: 'knowledge-base.admin.read' } };
  assert.throws(() => publicRoutesFromOpenApi(extraPath), /unapproved public API operation/);

  const extraMethod = structuredClone(publicApi);
  extraMethod.paths['/notes'].get = structuredClone(extraMethod.paths['/notes'].post);
  assert.throws(() => publicRoutesFromOpenApi(extraMethod), /unapproved public API operation/);

  const changedTemplate = structuredClone(publicApi);
  changedTemplate.paths['/notes/{note_id}/../admin'] = changedTemplate.paths['/notes/{note_id}'];
  delete changedTemplate.paths['/notes/{note_id}'];
  assert.throws(() => publicRoutesFromOpenApi(changedTemplate), /invalid public API path template/);

  const wrongIdentity = structuredClone(publicApi);
  wrongIdentity.paths['/notes'].post.operationId = 'knowledge-base.internal.run';
  assert.throws(() => publicRoutesFromOpenApi(wrongIdentity), /unapproved public API operation/);

  const missingOperation = structuredClone(publicApi);
  delete missingOperation.paths['/settings'].put;
  assert.throws(() => publicRoutesFromOpenApi(missingOperation), /omits an approved operation/);

  const wrongParameter = structuredClone(publicApi);
  wrongParameter.paths['/notes/{note_id}'].get.parameters[0].name = 'other_id';
  assert.throws(() => publicRoutesFromOpenApi(wrongParameter), /path parameters do not match/);

  const missingSecurity = structuredClone(publicApi);
  delete missingSecurity.paths['/notes'].post.security;
  assert.throws(() => publicRoutesFromOpenApi(missingSecurity), /must require bearer auth/);

  const wrongScheme = structuredClone(publicApi);
  wrongScheme.components.securitySchemes.bearerAuth.scheme = 'basic';
  assert.throws(() => publicRoutesFromOpenApi(wrongScheme), /must define bearer authentication/);
});

async function listen(handler) {
  const server = createServer(handler);
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    close: () => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
  };
}

async function backend(name) {
  return listen(async (request, response) => {
    const body = [];
    for await (const chunk of request) body.push(chunk);
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify({
      backend: name,
      path: request.url,
      method: request.method,
      authorization: request.headers.authorization,
      body: Buffer.concat(body).toString(),
    }));
  });
}

test('the dev page reads the current backend file for handshake and public API requests', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'lenso-dev-backend-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const first = await backend('first');
  const second = await backend('second');
  t.after(() => first.close());
  t.after(() => second.close());
  const urlFile = join(directory, 'backend-url');
  const middleware = createDevBackendMiddleware(urlFile);
  const frontend = await listen((request, response) => middleware(request, response, () => {
    response.writeHead(404);
    response.end('not proxied');
  }));
  t.after(() => frontend.close());

  await writeFile(urlFile, `${first.url}\n`);
  const initialHandshake = await fetch(`${frontend.url}/__lenso/backend`);
  assert.equal(initialHandshake.status, 200);
  assert.equal(initialHandshake.headers.get('content-length'), String(Buffer.byteLength(first.url)));
  assert.equal(await initialHandshake.text(), first.url);

  const created = await fetch(`${frontend.url}/notes`, {
    method: 'POST',
    headers: { authorization: 'Bearer example', 'content-type': 'application/json' },
    body: '{"title":"A note"}',
  });
  assert.deepEqual(await created.json(), {
    backend: 'first', path: '/notes', method: 'POST',
    authorization: 'Bearer example', body: '{"title":"A note"}',
  });

  await writeFile(urlFile, second.url);
  const currentHandshake = await fetch(`${frontend.url}/__lenso/backend`);
  assert.equal(await currentHandshake.text(), second.url);
  const currentRequest = await fetch(`${frontend.url}/settings`);
  assert.equal((await currentRequest.json()).backend, 'second');
});

test('the dev proxy exposes only the reference App public API paths', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'lenso-dev-backend-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const upstream = await backend('public');
  t.after(() => upstream.close());
  const urlFile = join(directory, 'backend-url');
  await writeFile(urlFile, upstream.url);
  const middleware = createDevBackendMiddleware(urlFile);
  const frontend = await listen((request, response) => middleware(request, response, () => {
    response.writeHead(404);
    response.end('not proxied');
  }));
  t.after(() => frontend.close());

  for (const [path, method] of [
    ['/notes/one', 'GET'], ['/jobs/process-next', 'POST'], ['/job-status/one', 'GET'],
    ['/settings', 'GET'], ['/settings', 'PUT'], ['/note-attachments/one', 'POST'],
  ]) {
    const response = await fetch(new URL(path, frontend.url), { method });
    assert.equal(response.status, 200, `${method} ${path}`);
    assert.equal((await response.json()).path, path);
  }
  for (const path of ['/admin', '/internal/metrics', '/notes/one/history', '/__lenso/secret']) {
    const response = await fetch(new URL(path, frontend.url));
    assert.equal(response.status, 404, path);
    assert.equal(await response.text(), 'not proxied');
  }
  for (const [path, method] of [['/notes', 'GET'], ['/jobs/process-next', 'GET'], ['/notes/one', 'POST']]) {
    const response = await fetch(new URL(path, frontend.url), { method });
    assert.equal(response.status, 404, `${method} ${path}`);
  }
});

test('a missing or unsafe backend URL cannot silently use a stale target', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'lenso-dev-backend-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const urlFile = join(directory, 'backend-url');
  const middleware = createDevBackendMiddleware(urlFile);
  const frontend = await listen((request, response) => middleware(request, response, () => {
    response.writeHead(404);
    response.end('not proxied');
  }));
  t.after(() => frontend.close());

  for (const value of [null, 'https://example.com', 'http://127.0.0.1:3001/private']) {
    if (value !== null) await writeFile(urlFile, value);
    for (const path of ['/__lenso/backend', '/settings']) {
      const response = await fetch(new URL(path, frontend.url));
      assert.equal(response.status, 503, `${value ?? 'missing'} ${path}`);
    }
  }
  await writeFile(urlFile, `http://127.0.0.1:3001${' '.repeat(512)}`);
  assert.equal((await fetch(`${frontend.url}/__lenso/backend`)).status, 503);

  const linkedFile = join(directory, 'linked-url');
  await writeFile(linkedFile, 'http://127.0.0.1:3001');
  await rm(urlFile);
  await symlink(linkedFile, urlFile);
  assert.equal((await fetch(`${frontend.url}/__lenso/backend`)).status, 503);

  await rm(urlFile);
  await writeFile(urlFile, 'http://[::1]:3001/\n');
  const ipv6 = await fetch(`${frontend.url}/__lenso/backend`);
  assert.equal(ipv6.status, 200);
  assert.equal(await ipv6.text(), 'http://[::1]:3001/');
});
