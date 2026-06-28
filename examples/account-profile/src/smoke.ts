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
    catalogEntry.name !== "account-profile-service" ||
    catalogEntry.source !== "service" ||
    catalogEntry.baseUrl !== "http://127.0.0.1:4120/lenso/service/v1"
  ) {
    throw new Error("catalog entry does not describe account-profile-service");
  }

  const manifest = await fetchJson(server.manifestUrl);
  if (
    manifest.name !== "account-profile-service" ||
    manifest.protocol !== "lenso.service.v1" ||
    manifest.version !== catalogEntry.version
  ) {
    throw new Error("manifest did not return account-profile-service");
  }
  const moduleManifest = manifest.modules?.find(
    (module) => module.name === "account-profile"
  );
  if (!moduleManifest || !moduleManifest.dependencies.includes("auth")) {
    throw new Error("service manifest did not provide the account-profile auth contract");
  }
  for (const capability of catalogEntry.capabilities) {
    if (!moduleManifest.capabilities.includes(capability)) {
      throw new Error(`manifest is missing catalog capability ${capability}`);
    }
  }
  const entities = moduleManifest.admin?.fallback_schema?.entities ?? [];
  for (const entityName of ["profiles", "organizations", "memberships"]) {
    if (!entities.some((entity) => entity.name === entityName)) {
      throw new Error(`manifest did not expose ${entityName}`);
    }
  }

  const status = await fetchJson(server.statusUrl ?? `${server.baseUrl}/status`);
  if (
    status.serviceName !== "account-profile-service" ||
    status.state !== "ready" ||
    !status.modules?.some((module) => module.name === "account-profile")
  ) {
    throw new Error(
      "status endpoint did not return account-profile-service readiness"
    );
  }

  const moduleBaseUrl = `${server.baseUrl}/modules/account-profile`;
  const created = await fetchJson(`${moduleBaseUrl}/profiles`, {
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

  const updated = await fetchJson(
    `${moduleBaseUrl}/admin/actions/upsert_profile`,
    {
      body: JSON.stringify({
        auth_user_id: "auth_user_2",
        display_name: "Sam Operations",
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  if (updated.result?.profile?.display_name !== "Sam Operations") {
    throw new Error("admin action did not update profile_auth_user_2");
  }

  const membership = await fetchJson(
    `${moduleBaseUrl}/organizations/org_1/memberships`,
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
    `${moduleBaseUrl}/profiles/profile_auth_user_2`
  );
  if (profile.profile?.display_name !== "Sam Operations") {
    throw new Error("HTTP detail route did not return updated profile");
  }

  const admin = await fetchJson(`${moduleBaseUrl}/admin/memberships`);
  const roles = admin.records?.map((record) => record.role) ?? [];
  if (!roles.includes("owner") || !roles.includes("admin")) {
    throw new Error("schema-admin endpoint did not return memberships");
  }

  console.log("Account Profile service smoke passed");
} finally {
  await server.close();
}
