#!/usr/bin/env node
import { serveSupportTicketModule } from "./module.ts";

await serveSupportTicketModule({
  port: Number(process.env.PORT ?? "4110"),
  onReady: ({ manifestUrl, statusUrl }) => {
    console.log(`Support Ticket service manifest: ${manifestUrl}`);
    console.log(`Support Ticket service status: ${statusUrl}`);
  },
});
