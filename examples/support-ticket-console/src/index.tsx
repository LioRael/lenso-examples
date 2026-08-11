import { defineConsoleManifest } from "@lenso/console-module-api";
import type { ConsoleModuleManifest } from "@lenso/console-module-api";
import "@lenso/console-tokens/stylex.css";
import "@lenso/console-ui/stylex.css";
import { defineConsoleModule, defineConsoleUiModule } from "@lenso/console-ui";

import manifestDefinition from "../console-module.json";
import { SupportTicketsPage } from "./page";

export const supportTicketConsoleModule = defineConsoleModule({
  id: "support/tickets",
  surfaces: [
    {
      area: "data",
      component: SupportTicketsPage,
      icon: "database",
      label: "Support tickets",
      navigation: {
        order: 20,
        workspace: {
          icon: "settings",
          id: "system",
          label: "System",
          localizedLabels: { "zh-CN": "系统" },
        },
      },
      path: "/support/tickets",
    },
  ],
});

export const supportTicketConsoleUiModule = defineConsoleUiModule({
  manifest: defineConsoleManifest(manifestDefinition as ConsoleModuleManifest),
  surfaces: { "support-tickets": SupportTicketsPage },
});

export default supportTicketConsoleUiModule;

export * from "./business-api";
export { SupportTicketsPage } from "./page";
