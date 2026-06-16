#!/usr/bin/env node
import { serveGrpcNotesModule } from "./module.mjs";

await serveGrpcNotesModule({
  port: Number(process.env.GRPC_PORT ?? "50051"),
  onReady: ({ baseUrl }) => {
    console.log(`gRPC Notes endpoint: ${baseUrl}`);
    console.log(`REMOTE_MODULES=grpc-notes=${baseUrl}`);
  },
});
