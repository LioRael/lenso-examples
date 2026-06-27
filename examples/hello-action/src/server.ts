#!/usr/bin/env node
import { serveHelloActionModule } from "./module.ts";

await serveHelloActionModule({
  port: Number(process.env.PORT ?? "4100"),
  onReady: ({ manifestUrl, statusUrl }) => {
    console.log(`Hello service manifest: ${manifestUrl}`);
    console.log(`Hello service status: ${statusUrl}`);
  },
});
