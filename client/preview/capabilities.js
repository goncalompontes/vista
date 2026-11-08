// Backend-agnostic capability demo for the browser UI preview.
//
// The same renderer consumes descriptors from either backend. No view branches
// on the backend name, OS or compositor; only the declared capability state
// decides whether a control is offered, disabled or denied. This mirrors the
// Rust contract in host/src/capability.rs and docs/fixtures/capabilities-v1.json.

const action = (id, label, options = {}) => ({
  id,
  label,
  supported: true,
  enabled: true,
  authorization: "granted",
  reason: null,
  parameters: {},
  ...options
});

export const backends = {
  niri: {
    label: "Niri (real capability set)",
    targetId: "w5",
    targetGeneration: "niri-boot1-w5",
    actions: [
      action("window.focus", "Focus window"),
      action("window.close", "Close window"),
      action("desktop.workspace.step", "Next workspace", {
        parameters: { direction: "next" }
      }),
      action("window.maximize.set", "Maximize", {
        supported: false,
        enabled: false,
        authorization: "required",
        reason: "Niri exposes fullscreen, not maximize"
      }),
      action("edit.copy", "Copy", {
        authorization: "denied",
        reason: "Clipboard grant was not approved"
      })
    ]
  },
  alternate: {
    label: "Simulated alternate backend",
    targetId: "w5",
    targetGeneration: "alt-session-1-w5",
    actions: [
      action("window.focus", "Focus window"),
      action("window.maximize.set", "Maximize"),
      action("desktop.overview.show", "Show overview"),
      action("window.minimize", "Minimize", {
        enabled: false,
        reason: "No managed session is selected"
      }),
      action("example.vendor.special", "Vendor action")
    ]
  }
};

export const backendIds = Object.keys(backends);

export function createCapabilityState(backendId = "niri") {
  const backend = backends[backendId];
  if (!backend) throw new Error(`unknown backend: ${backendId}`);
  return {
    backendId,
    label: backend.label,
    targetId: backend.targetId,
    targetGeneration: backend.targetGeneration,
    revision: 12,
    maximized: false,
    lastResult: null,
    actions: backend.actions.map(entry => ({ ...entry, parameters: { ...entry.parameters } }))
  };
}

/** The renderer uses this for every backend; there is no backend-specific view path. */
export function renderableActions(state) {
  return state.actions.map(entry => ({
    id: entry.id,
    label: entry.label,
    invokable:
      entry.supported &&
      entry.enabled &&
      entry.authorization === "granted",
    hint: !entry.supported
      ? "Unavailable on this backend"
      : !entry.enabled
        ? entry.reason || "Temporarily unavailable"
        : entry.authorization === "required"
          ? "Permission required"
          : entry.authorization === "denied" || entry.authorization === "revoked"
            ? entry.reason || "Permission denied"
            : null
  }));
}

function rejected(code, detail) {
  return { status: "rejected", errorCode: code, detail };
}

/**
 * Local stand-in for the host registry. It revalidates exactly the conditions
 * the host checks and returns a completed result or a structured rejection.
 */
export function invokeAction(state, actionId) {
  const entry = state.actions.find(candidate => candidate.id === actionId);
  if (!entry) return rejected("unsupported_action", "The host did not advertise this action.");
  if (!entry.supported) return rejected("unsupported_action", entry.reason || "Unsupported.");
  if (!entry.enabled) return rejected("backend_unavailable", entry.reason || "Unavailable.");
  if (entry.authorization !== "granted") {
    return rejected(
      entry.authorization === "required" ? "permission_required" : "permission_denied",
      entry.reason || "Permission is not granted."
    );
  }

  const next = {
    ...state,
    actions: state.actions.map(candidate => ({ ...candidate, parameters: { ...candidate.parameters } }))
  };
  let detail = `${entry.label} completed.`;

  switch (entry.id) {
    case "window.maximize.set":
      next.maximized = !next.maximized;
      next.actions = next.actions.map(candidate =>
        candidate.id === "window.maximize.set"
          ? { ...candidate, label: next.maximized ? "Restore" : "Maximize" }
          : candidate
      );
      detail = next.maximized ? "Window maximized." : "Window restored.";
      break;
    case "window.focus":
      detail = "Window focused.";
      break;
    case "window.close":
      detail = "Window close requested.";
      break;
    case "desktop.overview.show":
      detail = "Overview shown.";
      break;
    case "desktop.workspace.step":
      detail = "Workspace changed.";
      break;
    case "example.vendor.special":
      detail = "Vendor extension invoked.";
      break;
    default:
      detail = `${entry.label} completed.`;
  }

  return {
    status: "completed",
    errorCode: null,
    detail,
    state: { ...next, revision: state.revision + 1, lastResult: { status: "completed", detail } }
  };
}

export function applyResult(state, result) {
  if (result.status === "completed") return result.state;
  return {
    ...state,
    lastResult: {
      status: result.status,
      errorCode: result.errorCode,
      detail: result.detail
    }
  };
}
