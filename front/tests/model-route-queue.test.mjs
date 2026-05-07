import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  addModelRouteCandidate,
  buildModelRoutePayload,
  createEditingCandidate,
  createModelRouteTemplate,
  mapModelRouteDetailToEditingRoute,
  moveModelRouteCandidate,
  removeModelRouteCandidate,
  setModelRouteCandidateEnabled,
  setModelRouteCandidateModel,
  setModelRouteCandidateProvider,
  validateModelRouteEditor,
} from "../src/pages/model-route/composables/modelRouteQueue.ts";

const ROOT = new URL("../", import.meta.url);

test("model route detail maps candidates by priority order", () => {
  const route = mapModelRouteDetailToEditingRoute({
    route: {
      id: 8,
      route_name: "gateway-auto",
      description: null,
      is_enabled: true,
      expose_in_models: false,
    },
    candidates: [
      {
        candidate: {
          id: 42,
          route_id: 8,
          model_id: 101,
          priority: 20,
          is_enabled: false,
        },
        provider_id: 2,
        provider_key: "anthropic",
        model_name: "claude-3-5-sonnet",
        real_model_name: null,
        model_is_enabled: true,
      },
      {
        candidate: {
          id: 41,
          route_id: 8,
          model_id: 100,
          priority: 0,
          is_enabled: true,
        },
        provider_id: 1,
        provider_key: "openai",
        model_name: "gpt-4o",
        real_model_name: "gpt-4o-2024-08-06",
        model_is_enabled: true,
      },
    ],
  });

  assert.equal(route.id, 8);
  assert.equal(route.description, "");
  assert.deepEqual(
    route.candidates.map((candidate) => candidate.model_id),
    ["100", "101"],
  );
  assert.deepEqual(
    route.candidates.map((candidate) => candidate.priority),
    [0, 20],
  );
});

test("model route queue add, move, remove, and enable operations keep normalized priorities", () => {
  const first = createEditingCandidate({
    local_id: "a",
    provider_id: "1",
    model_id: "10",
  });
  const second = createEditingCandidate({
    local_id: "b",
    provider_id: "2",
    model_id: "20",
  });
  const third = createEditingCandidate({
    local_id: "c",
    provider_id: "3",
    model_id: "30",
  });

  let candidates = addModelRouteCandidate([first, second], third);
  assert.deepEqual(
    candidates.map((candidate) => candidate.priority),
    [0, 10, 20],
  );

  candidates = moveModelRouteCandidate(candidates, 2, -1);
  assert.deepEqual(
    candidates.map((candidate) => candidate.local_id),
    ["a", "c", "b"],
  );
  assert.deepEqual(
    candidates.map((candidate) => candidate.priority),
    [0, 10, 20],
  );

  candidates = setModelRouteCandidateEnabled(candidates, 1, false);
  assert.equal(candidates[1].is_enabled, false);

  candidates = removeModelRouteCandidate(candidates, 0);
  assert.deepEqual(
    candidates.map((candidate) => [candidate.local_id, candidate.priority]),
    [
      ["c", 0],
      ["b", 10],
    ],
  );
});

test("model route provider changes clear model selection and payload follows queue order", () => {
  const route = createModelRouteTemplate(() => "local-1");
  route.route_name = "  gateway-fast  ";
  route.description = "  ";
  route.candidates = [
    createEditingCandidate({
      local_id: "a",
      provider_id: "1",
      model_id: "10",
    }),
    createEditingCandidate({
      local_id: "b",
      provider_id: "2",
      model_id: "20",
      is_enabled: false,
    }),
  ];

  route.candidates = setModelRouteCandidateProvider(route.candidates, 0, "3");
  assert.equal(route.candidates[0].provider_id, "3");
  assert.equal(route.candidates[0].model_id, null);

  route.candidates = setModelRouteCandidateModel(route.candidates, 0, "30");
  assert.equal(route.candidates[0].model_id, "30");
  assert.deepEqual(validateModelRouteEditor(route), {
    valid: true,
    issue: null,
  });

  assert.deepEqual(buildModelRoutePayload(route), {
    route_name: "gateway-fast",
    description: null,
    is_enabled: true,
    expose_in_models: true,
    candidates: [
      { model_id: 30, priority: 0, is_enabled: true },
      { model_id: 20, priority: 10, is_enabled: false },
    ],
  });
});

test("model route validation reports missing and duplicate queue fields", () => {
  const route = createModelRouteTemplate(() => "local-1");
  assert.deepEqual(validateModelRouteEditor(route), {
    valid: false,
    issue: "route_name_required",
  });

  route.route_name = "gateway";
  route.candidates = [];
  assert.deepEqual(validateModelRouteEditor(route), {
    valid: false,
    issue: "candidate_required",
  });

  route.candidates = [createEditingCandidate({ local_id: "a" })];
  assert.deepEqual(validateModelRouteEditor(route), {
    valid: false,
    issue: "candidate_model_required",
  });

  route.candidates = [
    createEditingCandidate({ local_id: "a", model_id: "10" }),
    createEditingCandidate({ local_id: "b", model_id: "10" }),
  ];
  assert.deepEqual(validateModelRouteEditor(route), {
    valid: false,
    issue: "duplicate_candidate",
  });
});

test("model route page uses page-local entry point", async () => {
  const routerSource = await readFile(new URL("src/router/index.ts", ROOT), "utf8");

  assert.match(routerSource, /pages\/model-route\/ModelRoutePage\.vue/);
  assert.equal(routerSource.includes("@/pages/ModelRoute.vue"), false);
});
