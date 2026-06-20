#!/usr/bin/env node
import { serveSupportTicketModule } from "./module.ts";

await serveSupportTicketModule({
  port: Number(process.env.PORT ?? "4110"),
  onReady: ({ manifestUrl }) => {
    console.log(`Support Ticket manifest: ${manifestUrl}`);
  },
});
