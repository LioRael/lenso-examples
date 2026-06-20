#!/usr/bin/env node
import { readFile } from "node:fs/promises";

import { serveAccountProfileModule } from "./module.ts";

const fetchJson = async (url, init) => {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  return response.json();
};

const server = await serveAccountProfileModule({ port: 0 });

try {
  const catalogEntry = JSON.parse(
    await readFile(new URL("../catalog-entry.json", import.meta.url), "utf8")
  );
  if (
    catalogEntry.name !== "account-profile" ||
    catalogEntry.source !== "remote" ||
    catalogEntry.baseUrl !== "http://127.0.0.1:4120/lenso/module/v1"
  ) {
    throw new Error("catalog entry does not describe account-profile");
  }

  const manifest = await fetchJson(server.manifestUrl);
  if (
    manifest.name !== "account-profile" ||
    manifest.version !== catalogEntry.version ||
    !manifest.dependencies.includes("auth")
  ) {
    throw new Error("manifest did not return the account-profile auth contract");
  }
  for (const capability of catalogEntry.capabilities) {
    if (!manifest.capabilities.includes(capability)) {
      throw new Error(`manifest is missing catalog capability ${capability}`);
    }
  }
  const entities = manifest.admin?.fallback_schema?.entities ?? [];
  for (const entityName of ["profiles", "organizations", "memberships"]) {
    if (!entities.some((entity) => entity.name === entityName)) {
      throw new Error(`manifest did not expose ${entityName}`);
    }
  }

  const created = await fetchJson(`${server.baseUrl}/profiles`, {
    body: JSON.stringify({
      auth_user_id: "auth_user_2",
      contact_email: "sam@example.com",
      display_name: "Sam Ops",
    }),
    headers: { "content-type": "application/json" },
    method: "POST",
  });
  if (created.profile?.id !== "profile_auth_user_2") {
    throw new Error("HTTP profile route did not create profile_auth_user_2");
  }

  const updated = await fetchJson(`${server.baseUrl}/admin/actions/upsert_profile`, {
    body: JSON.stringify({
      auth_user_id: "auth_user_2",
      display_name: "Sam Operations",
    }),
    headers: { "content-type": "application/json" },
    method: "POST",
  });
  if (updated.result?.profile?.display_name !== "Sam Operations") {
    throw new Error("admin action did not update profile_auth_user_2");
  }

  const membership = await fetchJson(
    `${server.baseUrl}/organizations/org_1/memberships`,
    {
      body: JSON.stringify({
        profile_id: "profile_auth_user_2",
        role: "admin",
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  if (membership.membership?.role !== "admin") {
    throw new Error("HTTP membership route did not add admin membership");
  }

  const profile = await fetchJson(
    `${server.baseUrl}/profiles/profile_auth_user_2`
  );
  if (profile.profile?.display_name !== "Sam Operations") {
    throw new Error("HTTP detail route did not return updated profile");
  }

  const admin = await fetchJson(`${server.baseUrl}/admin/memberships`);
  const roles = admin.records?.map((record) => record.role) ?? [];
  if (!roles.includes("owner") || !roles.includes("admin")) {
    throw new Error("schema-admin endpoint did not return memberships");
  }

  console.log("Account Profile remote module smoke passed");
} finally {
  await server.close();
}
