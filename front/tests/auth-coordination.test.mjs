import test from "node:test";
import assert from "node:assert/strict";

import {
  MANAGER_AUTH_CHANNEL_NAME,
  createAuthCoordination,
} from "../src/services/authCoordination.ts";

function channelHarness() {
  let listener = null;
  const sent = [];
  let closed = false;
  return {
    channel: {
      addEventListener(type, callback) {
        assert.equal(type, "message");
        listener = callback;
      },
      removeEventListener(type, callback) {
        assert.equal(type, "message");
        if (listener === callback) listener = null;
      },
      postMessage(event) {
        sent.push(event);
      },
      close() {
        closed = true;
      },
    },
    emit(data) {
      listener?.({ data });
    },
    get closed() {
      return closed;
    },
    sent,
  };
}

function lifecycleHarness() {
  let focus = null;
  let visibility = null;
  const documentTarget = {
    visibilityState: "visible",
    addEventListener(type, listener) {
      assert.equal(type, "visibilitychange");
      visibility = listener;
    },
    removeEventListener(type, listener) {
      assert.equal(type, "visibilitychange");
      if (visibility === listener) visibility = null;
    },
  };
  const windowTarget = {
    addEventListener(type, listener) {
      assert.equal(type, "focus");
      focus = listener;
    },
    removeEventListener(type, listener) {
      assert.equal(type, "focus");
      if (focus === listener) focus = null;
    },
  };
  return {
    documentTarget,
    windowTarget,
    focus: () => focus?.(),
    visibility: () => visibility?.(),
    hasListeners: () => focus !== null && visibility !== null,
  };
}

test("coordination emits exact versioned events without credentials", () => {
  const broadcast = channelHarness();
  const lifecycle = lifecycleHarness();
  const coordination = createAuthCoordination(
    {
      recoverAccess: async () => "access-not-broadcast",
      invalidateAccessRecovery: () => {},
      revokeLocalSession: () => {},
    },
    {
      createChannel: () => broadcast.channel,
      windowTarget: lifecycle.windowTarget,
      documentTarget: lifecycle.documentTarget,
    },
  );

  coordination.announceSessionChanged();
  coordination.announceSessionRevoked();

  assert.deepEqual(broadcast.sent, [
    { schema_version: 1, type: "session_changed" },
    { schema_version: 1, type: "session_revoked" },
  ]);
  assert.equal(JSON.stringify(broadcast.sent).includes("access-not-broadcast"), false);
  assert.equal(MANAGER_AUTH_CHANNEL_NAME, "cyder-manager-auth-v1");
});

test("received changed event recovers independently and revoked event only clears local state", async () => {
  const broadcast = channelHarness();
  const lifecycle = lifecycleHarness();
  let recoverCalls = 0;
  let invalidationCalls = 0;
  let revokeCalls = 0;
  const coordination = createAuthCoordination(
    {
      recoverAccess: async () => {
        recoverCalls += 1;
        return "access-local";
      },
      invalidateAccessRecovery: () => {
        invalidationCalls += 1;
      },
      revokeLocalSession: () => {
        revokeCalls += 1;
      },
    },
    {
      createChannel: () => broadcast.channel,
      windowTarget: lifecycle.windowTarget,
      documentTarget: lifecycle.documentTarget,
    },
  );

  broadcast.emit({ schema_version: 1, type: "session_changed" });
  await Promise.resolve();
  assert.equal(recoverCalls, 1);
  assert.equal(invalidationCalls, 1);
  assert.equal(revokeCalls, 0);

  broadcast.emit({ schema_version: 1, type: "session_revoked" });
  assert.equal(revokeCalls, 1);

  lifecycle.documentTarget.visibilityState = "hidden";
  broadcast.emit({ schema_version: 1, type: "session_changed" });
  await Promise.resolve();
  assert.equal(invalidationCalls, 2);
  assert.equal(recoverCalls, 1);

  for (const invalid of [
    { schema_version: 2, type: "session_revoked" },
    { schema_version: 1, type: "session_revoked", access_token: "forbidden" },
    { type: "session_changed" },
  ]) {
    broadcast.emit(invalid);
  }
  assert.equal(recoverCalls, 1);
  assert.equal(invalidationCalls, 2);
  assert.equal(revokeCalls, 1);

  coordination.dispose();
  assert.equal(broadcast.closed, true);
  assert.equal(lifecycle.hasListeners(), false);
});

test("focus and visible lifecycle events converge even without BroadcastChannel", async () => {
  const lifecycle = lifecycleHarness();
  let recoverCalls = 0;
  const coordination = createAuthCoordination(
    {
      recoverAccess: async () => {
        recoverCalls += 1;
        return "access-local";
      },
      invalidateAccessRecovery: () => {},
      revokeLocalSession: () => {},
    },
    {
      createChannel: () => null,
      windowTarget: lifecycle.windowTarget,
      documentTarget: lifecycle.documentTarget,
    },
  );

  lifecycle.focus();
  lifecycle.visibility();
  await Promise.resolve();
  assert.equal(recoverCalls, 2);

  lifecycle.documentTarget.visibilityState = "hidden";
  lifecycle.focus();
  lifecycle.visibility();
  await Promise.resolve();
  assert.equal(recoverCalls, 2);

  lifecycle.documentTarget.visibilityState = "visible";
  lifecycle.visibility();
  await Promise.resolve();
  assert.equal(recoverCalls, 3);

  coordination.dispose();
});
