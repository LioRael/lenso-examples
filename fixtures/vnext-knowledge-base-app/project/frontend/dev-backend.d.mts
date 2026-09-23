import type { IncomingMessage, ServerResponse } from 'node:http';

export declare function createDevBackendMiddleware(
  urlFile: string | undefined,
): (request: IncomingMessage, response: ServerResponse, next: () => void) => void;
