#if !SKIP
import Foundation

/// Client-side capability state and action reconciliation.
///
/// This is transport-independent: the UI creates a request from the current
/// snapshot, sends it through whatever authenticated channel exists, and applies
/// the host's result here. It never executes an action locally and never
/// invents a capability that the host did not advertise.
public struct CapabilitySession: Equatable, Sendable {
    public private(set) var snapshot: CapabilitySnapshot
    public private(set) var lastResult: ActionResultPayload?
    public private(set) var pending: [String: String]
    private var nextRequestNumber: UInt64

    public init(snapshot: CapabilitySnapshot, startingRequestNumber: UInt64 = 1) {
        self.snapshot = snapshot
        self.lastResult = nil
        self.pending = [:]
        self.nextRequestNumber = max(1, startingRequestNumber)
    }

    public var revision: UInt64 {
        snapshot.capabilityRevision
    }

    public var selectableActions: [CapabilityAction] {
        snapshot.actions.filter(\.isSelectable)
    }

    public var pendingRequestIDs: [String] {
        pending.keys.sorted()
    }

    public func action(id: String) -> CapabilityAction? {
        snapshot.action(id: id)
    }

    /// Builds a request for one currently selectable action. Returns `nil` for
    /// unsupported, disabled, denied or unknown actions so a view cannot send a
    /// request the host would immediately reject.
    public mutating func makeRequest(
        actionID: String,
        parameters: [String: JSONValue] = [:]
    ) -> ActionRequestPayload? {
        guard let action = snapshot.action(id: actionID), action.isSelectable else {
            return nil
        }
        let requestID = "r\(nextRequestNumber)"
        nextRequestNumber += 1
        let request = ActionRequestPayload(
            v: snapshot.v,
            messageType: "action.invoke",
            id: requestID,
            connectionEpoch: snapshot.connectionEpoch,
            sessionId: snapshot.sessionId,
            grantEpoch: snapshot.grantEpoch,
            targetId: snapshot.targetId,
            targetGeneration: snapshot.targetGeneration,
            capabilityRevision: snapshot.capabilityRevision,
            action: action.id,
            schemaVersion: action.schemaVersion,
            parameters: parameters
        )
        pending[requestID] = action.id
        return request
    }

    /// Applies one authoritative result. Returns `false` for a result that does
    /// not belong to this connection, an old revision, or an unknown request;
    /// such a result must not update the UI.
    @discardableResult
    public mutating func apply(_ result: ActionResultPayload) -> Bool {
        guard result.connectionEpoch == snapshot.connectionEpoch else {
            return false
        }
        if let revision = result.capabilityRevision, revision < snapshot.capabilityRevision {
            return false
        }
        guard let actionID = pending.removeValue(forKey: result.id) else {
            return false
        }
        lastResult = result

        guard result.status == .completed else {
            // Rejected, failed and outcome_unknown are surfaced honestly. None
            // of them causes an automatic replay.
            return true
        }

        var actions = snapshot.actions
        if actionID == "window.maximize.set",
           case .object(let state)? = result.state,
           case .bool(let maximized)? = state["maximized"]
        {
            actions = actions.map { action in
                guard action.id == actionID else { return action }
                return CapabilityAction(
                    id: action.id,
                    schemaVersion: action.schemaVersion,
                    supported: action.supported,
                    enabled: action.enabled,
                    reason: action.reason,
                    authorization: action.authorization,
                    parameters: action.parameters,
                    presentation: ActionPresentation(
                        label: maximized ? "Restore" : "Maximize",
                        icon: action.presentation.icon,
                        shortcut: action.presentation.shortcut
                    ),
                    suggestedBindings: action.suggestedBindings
                )
            }
        }

        snapshot = CapabilitySnapshot(
            v: snapshot.v,
            messageType: snapshot.messageType,
            connectionEpoch: snapshot.connectionEpoch,
            sessionId: snapshot.sessionId,
            targetId: snapshot.targetId,
            targetGeneration: snapshot.targetGeneration,
            capabilityRevision: result.capabilityRevision ?? snapshot.capabilityRevision,
            grantEpoch: snapshot.grantEpoch,
            actions: actions
        )
        return true
    }

    /// A reconnect means a new connection/epoch and a fresh snapshot; queued
    /// requests and old results must not be replayed.
    public mutating func reconnect(with snapshot: CapabilitySnapshot) {
        self = CapabilitySession(snapshot: snapshot)
    }
}

#endif
