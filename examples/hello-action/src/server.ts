#!/usr/bin/env node
import { serveHelloActionModule } from "./module.ts";

await serveHelloActionModule({
  port: Number(process.env.PORT ?? "4100"),
  onReady: ({ manifestUrl }) => {
    console.log(`Hello Action manifest: ${manifestUrl}`);
  },
});
