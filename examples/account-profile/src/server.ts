#!/usr/bin/env node
import { serveAccountProfileModule } from "./module.ts";

await serveAccountProfileModule({
  port: Number(process.env.PORT ?? "4120"),
  onReady: ({ manifestUrl, statusUrl }) => {
    console.log(`Account Profile service manifest: ${manifestUrl}`);
    console.log(`Account Profile service status: ${statusUrl}`);
  },
});
