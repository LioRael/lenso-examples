import {
  actionTextField,
  adminAction,
  adminSchema,
  declarativeCustom,
  declarativePage,
  declarativeSection,
  defineRemoteModule,
  defineSchemaEntity,
  entityTable,
  getRoute,
  postRoute,
  serveRemoteModule,
  textField,
  timestampField,
} from "@lenso/remote-module-kit";

const profiles = [
  {
    auth_user_id: "auth_user_1",
    contact_email: "avery@example.com",
    created_at: "2026-06-21T00:00:00Z",
    display_name: "Avery Product",
    id: "profile_auth_user_1",
    updated_at: "2026-06-21T00:00:00Z",
  },
];

const organizations = [
  {
    created_at: "2026-06-21T00:00:00Z",
    id: "org_1",
    name: "Acme Support",
    updated_at: "2026-06-21T00:00:00Z",
  },
];

const memberships = [
  {
    created_at: "2026-06-21T00:00:00Z",
    id: "membership_1",
    organization_id: "org_1",
    profile_id: "profile_auth_user_1",
    role: "owner",
  },
];

const profileReadCapability = "account_profile.profiles.read";
const profileWriteCapability = "account_profile.profiles.write";
const organizationReadCapability = "account_profile.organizations.read";
const organizationWriteCapability = "account_profile.organizations.write";

const profileEntity = defineSchemaEntity({
  fields: [
    textField("id", { label: "ID" }),
    textField("auth_user_id", { label: "Auth User ID" }),
    textField("display_name", { label: "Display Name" }),
    textField("contact_email", { label: "Contact Email" }),
    timestampField("created_at", { label: "Created At" }),
    timestampField("updated_at", { label: "Updated At" }),
  ],
  label: "Profiles",
  name: "profiles",
  readCapability: profileReadCapability,
});

const organizationEntity = defineSchemaEntity({
  fields: [
    textField("id", { label: "ID" }),
    textField("name", { label: "Name" }),
    timestampField("created_at", { label: "Created At" }),
    timestampField("updated_at", { label: "Updated At" }),
  ],
  label: "Organizations",
  name: "organizations",
  readCapability: organizationReadCapability,
});

const membershipEntity = defineSchemaEntity({
  fields: [
    textField("id", { label: "ID" }),
    textField("profile_id", { label: "Profile ID" }),
    textField("organization_id", { label: "Organization ID" }),
    textField("role", { label: "Role" }),
    timestampField("created_at", { label: "Created At" }),
  ],
  label: "Memberships",
  name: "memberships",
  readCapability: organizationReadCapability,
});

const baseManifest = defineRemoteModule({
  admin: declarativeCustom({
    actions: [
      adminAction("upsert_profile", {
        capability: profileWriteCapability,
        inputFields: [
          actionTextField("auth_user_id", {
            label: "Auth User ID",
            required: true,
          }),
          actionTextField("display_name", {
            label: "Display Name",
            required: true,
          }),
          actionTextField("contact_email", { label: "Contact Email" }),
        ],
        label: "Upsert profile",
      }),
    ],
    fallbackSchema: adminSchema([
      profileEntity,
      organizationEntity,
      membershipEntity,
    ]),
    pages: [
      declarativePage("profiles", {
        sections: [
          declarativeSection("records", {
            component: entityTable("profiles"),
            label: "Profiles",
          }),
        ],
      }),
      declarativePage("organizations", {
        sections: [
          declarativeSection("records", {
            component: entityTable("organizations"),
            label: "Organizations",
          }),
          declarativeSection("memberships", {
            component: entityTable("memberships"),
            label: "Memberships",
          }),
        ],
      }),
    ],
  }),
  capabilities: [
    profileReadCapability,
    profileWriteCapability,
    organizationReadCapability,
    organizationWriteCapability,
  ],
  httpRoutes: [
    getRoute("/profiles/{id}", {
      capability: profileReadCapability,
      displayName: "Get profile",
      storyTitle: "Account profile viewed",
    }),
    postRoute("/profiles", {
      capability: profileWriteCapability,
      displayName: "Upsert profile",
      storyTitle: "Account profile updated",
    }),
    getRoute("/organizations/{id}", {
      capability: organizationReadCapability,
      displayName: "Get organization",
      storyTitle: "Organization viewed",
    }),
    postRoute("/organizations/{id}/memberships", {
      capability: organizationWriteCapability,
      displayName: "Add membership",
      storyTitle: "Organization membership added",
    }),
  ],
  name: "account-profile",
  version: "0.1.0",
});

export const manifest = {
  ...baseManifest,
  dependencies: ["auth"],
  // ponytail: kit 0.1.1 emits an empty runtime surface; omit it until a function exists.
  runtime: undefined,
};

const now = () => new Date().toISOString();

const textOrDefault = (value, fallback) =>
  typeof value === "string" && value.trim() ? value.trim() : fallback;

const list = (records, limit) => ({
  next_cursor: null,
  records: records.slice(0, limit),
});

const profileIdFor = (authUserId) => `profile_${authUserId}`;

const findProfile = (id) => profiles.find((profile) => profile.id === id);
const findOrganization = (id) =>
  organizations.find((organization) => organization.id === id);

const upsertProfile = (input = {}) => {
  const authUserId = textOrDefault(input.auth_user_id, "auth_user_new");
  const id = profileIdFor(authUserId);
  const timestamp = now();
  const existing = findProfile(id);
  if (existing) {
    existing.contact_email = textOrDefault(
      input.contact_email,
      existing.contact_email
    );
    existing.display_name = textOrDefault(
      input.display_name,
      existing.display_name
    );
    existing.updated_at = timestamp;
    return existing;
  }
  const profile = {
    auth_user_id: authUserId,
    contact_email: textOrDefault(input.contact_email, ""),
    created_at: timestamp,
    display_name: textOrDefault(input.display_name, "Unnamed user"),
    id,
    updated_at: timestamp,
  };
  profiles.push(profile);
  return profile;
};

const addMembership = (organizationId, input = {}) => {
  const organization = findOrganization(organizationId);
  const profile = findProfile(input.profile_id);
  if (!organization || !profile) {
    return undefined;
  }
  const membership = {
    created_at: now(),
    id: `membership_${memberships.length + 1}`,
    organization_id: organization.id,
    profile_id: profile.id,
    role: textOrDefault(input.role, "member"),
  };
  memberships.push(membership);
  return membership;
};

const data = {
  memberships: {
    detail: async (id) =>
      memberships.find((membership) => membership.id === id),
    list: async ({ limit }) => list(memberships, limit),
  },
  organizations: {
    detail: async (id) => findOrganization(id),
    list: async ({ limit }) => list(organizations, limit),
  },
  profiles: {
    detail: async (id) => findProfile(id),
    list: async ({ limit }) => list(profiles, limit),
  },
};

export const serveAccountProfileModule = async (options = {}) =>
  serveRemoteModule(manifest, {
    actions: {
      upsert_profile: ({ input }) => ({ profile: upsertProfile(input) }),
    },
    data,
    http: {
      "GET /organizations/{id}": ({ params }) => ({
        organization: findOrganization(params.id),
      }),
      "GET /profiles/{id}": ({ params }) => ({ profile: findProfile(params.id) }),
      "POST /organizations/{id}/memberships": ({ body, params }) => ({
        body: { membership: addMembership(params.id, body) },
        statusCode: 201,
      }),
      "POST /profiles": ({ body }) => ({
        body: { profile: upsertProfile(body) },
        statusCode: 201,
      }),
    },
    onReady: options.onReady,
    port: options.port ?? 4120,
  });
