import Foundation

/// Only authenticated adapters may construct a remote event. Every remote
/// callback carries its original connection, never the connection at delivery.
public enum ClientEvent: Equatable, Sendable {
    case discovered(String)
    case pairingPending
    case pairingRejected(String)
    case authenticated(ClientConnectionID)
    case remote(ClientConnectionID, ClientHostEvent)
    case intent(ClientIntent)
    case tick(UInt64)
    case backgrounded
    case foregrounded
}

public enum ClientHostEvent: Equatable, Sendable {
    case catalogSnapshot(ClientRevision, [ClientTarget])
    case catalogDelta(ClientRevision, ClientRevision, [ClientTarget], [ClientTarget])
    case capabilitySnapshot(ClientRevision, [String], [ClientAction])
    case capabilityDelta(ClientRevision, ClientRevision, [String])
    case selectTarget(ClientTarget)
    case permission(String, ClientPermission, ClientInputScope, ClientTarget?)
    case captureApproved(ClientTarget, String, ClientTarget?)
    case captureRejected(String)
    case consentCancelled
    case rtcConnected
    case firstFrame(ClientTarget, UInt64, UInt64)
    case decoderWaiting(ClientTarget, UInt64)
    case geometry(ClientTarget, UInt64)
    case lease(ClientLease)
    case leaseRevoked(String)
    case targetClosed(ClientTarget)
    case actionResult(String, ClientTarget, ClientRevision, Bool)
    case disconnected
    case sessionFailed(ClientTarget)
    case audioPlaying(Bool)
}

public enum ClientIntent: Equatable, Sendable {
    case openPicker(String)
    case invoke(String, String)
    case keyDown(Int)
    /// Coordinates already transformed by the platform's geometry adapter.
    case absolutePointer(Double, Double, UInt64, Bool)
    case inputMode(String)
    case clipboard(Bool)
    case releaseControl
    case muteAudio
}

public enum ClientEffect: Equatable, Sendable {
    case requestCatalog
    case requestCapabilities
    case requestKeyframe
    case requestGeometry
    case releaseAll
    case openPicker(String)
    case action(String, String, ClientTarget, ClientRevision)
    case keyDown(Int)
    case absolutePointer(Double, Double, UInt64)
    case clipboard(Bool)
    case ignoredStaleEvent
}

public struct ClientReduction: Equatable, Sendable {
    public let state: ClientAppState
    public let effects: [ClientEffect]
}

public enum ClientReducer {
    public static func reduce(_ previous: ClientAppState, _ event: ClientEvent) -> ClientReduction {
        transition(previous, event)
    }

    /// Named entry point avoids Skip/Kotlin's collection reduce overload rewrite.
    public static func transition(_ previous: ClientAppState, _ event: ClientEvent) -> ClientReduction {
        var state = previous
        var effects: [ClientEffect] = []
        switch event {
        case .discovered(let name): state.hostName = name
        case .pairingPending:
            state = ClientAppState()
            state.trust = .pairing
        case .pairingRejected(let reason):
            state = ClientAppState()
            state.trust = .rejected
            state.warning = reason == "HOST_IDENTITY_MISMATCH" ? "Host identity changed or does not match" : reason
            effects = [.releaseAll]
        case .authenticated(let connection):
            // Duplicate auth must not erase state or refresh expired grants.
            if state.connection != connection {
                let name = state.hostName
                state = ClientAppState()
                state.hostName = name
                state.trust = .verified
                state.connection = connection
                effects = [.releaseAll, .requestCatalog, .requestCapabilities, .requestKeyframe]
            }
        case .tick(let time):
            state.now = max(state.now, time)
            if let lease = state.lease, lease.expiresAt <= state.now {
                release(&state, &effects)
            }
        case .backgrounded:
            state.foreground = false
            release(&state, &effects)
        case .foregrounded: state.foreground = true
        case .intent(let intent): route(intent, &state, &effects)
        case .remote(let connection, let hostEvent):
            guard state.authenticated && state.connection == connection else {
                return ClientReduction(state: state, effects: [.ignoredStaleEvent])
            }
            apply(hostEvent, &state, &effects)
        }
        return ClientReduction(state: state, effects: effects)
    }

    private static func release(_ state: inout ClientAppState, _ effects: inout [ClientEffect]) {
        state.lease = nil
        state.heldKeys = []
        effects.append(.releaseAll)
    }

    private static func current(_ target: ClientTarget, _ state: ClientAppState) -> Bool {
        state.target == target && target.generation > (state.closedTargets[target.id] ?? UInt64(0))
    }

    private static func apply(_ event: ClientHostEvent, _ state: inout ClientAppState, _ effects: inout [ClientEffect]) {
        switch event {
        case .catalogSnapshot(let revision, let items):
            if let previousRevision = state.catalogRevision {
                if revision.rawValue < previousRevision.rawValue { return }
                if revision == previousRevision && state.catalogFresh { return }
            }
            state.catalogRevision = revision
            state.catalogFresh = true
            state.catalog = items.filter { $0.generation > (state.closedTargets[$0.id] ?? UInt64(0)) }
        case .catalogDelta(let base, let revision, let upsert, let removed):
            guard state.catalogFresh && base == state.catalogRevision && revision.rawValue > base.rawValue && revision.rawValue - base.rawValue == UInt64(1) else {
                state.catalogFresh = false
                effects.append(.requestCatalog)
                return
            }
            state.catalogRevision = revision
            state.catalog.removeAll { item in removed.contains(item) || upsert.contains(where: { $0.id == item.id }) }
            state.catalog += upsert.filter { $0.generation > (state.closedTargets[$0.id] ?? UInt64(0)) }
        case .capabilitySnapshot(let revision, let features, let actions):
            if let previousRevision = state.capabilityRevision {
                if revision.rawValue < previousRevision.rawValue { return }
                if revision == previousRevision && state.capabilitiesFresh { return }
            }
            state.capabilityRevision = revision
            state.capabilitiesFresh = true
            state.features = features
            state.actions = actions
            state.pending = [:]
            if !features.contains("input.pointer") { release(&state, &effects) }
        case .capabilityDelta(let base, let revision, let removed):
            guard state.capabilitiesFresh && base == state.capabilityRevision && revision.rawValue > base.rawValue && revision.rawValue - base.rawValue == UInt64(1) else {
                state.capabilitiesFresh = false
                state.pending = [:]
                release(&state, &effects)
                effects.append(.requestCapabilities)
                return
            }
            state.capabilityRevision = revision
            state.actions.removeAll { removed.contains($0.id) }
            state.features.removeAll { removed.contains($0) }
            state.pending = [:]
            if removed.contains("input.pointer") { release(&state, &effects) }
        case .selectTarget(let target):
            guard target.generation > (state.closedTargets[target.id] ?? UInt64(0)) else { return }
            if state.target != target {
                release(&state, &effects)
                state.target = target
                state.permissions = [:]
                state.inputScope = .unknown
                state.pending = [:]
                state.lastConfirmedAction = nil
                state.media = .none
                state.consent = .none
                state.sourceKind = nil
                state.verifiedCatalogTarget = nil
                state.firstFrame = false
                state.decoderGeneration = UInt64(0)
                state.geometryEpoch = UInt64(0)
                state.geometryReady = false
                state.audioPlaying = false
            }
        case .permission(let name, let permission, let scope, let target):
            guard name == "catalog.read" || target.map({ current($0, state) }) == true else { return }
            state.permissions[name] = permission
            if name == "input.control" {
                state.inputScope = scope
                if permission != .granted { release(&state, &effects) }
            }
            if name == "video.view" && permission != .granted {
                state.media = .none
                state.firstFrame = false
                release(&state, &effects)
            }
            if name == "audio.listen" && permission != .granted { state.audioPlaying = false }
        case .captureApproved(let target, let kind, let binding):
            guard current(target, state), state.consent == .waiting,
                  state.requestedSourceKind == nil || state.requestedSourceKind == kind else { return }
            state.consent = .approved
            state.sourceKind = kind
            state.verifiedCatalogTarget = binding.flatMap { state.catalog.contains($0) ? $0 : nil }
            state.media = .waitingForFirstFrame
        case .captureRejected(let reason):
            state.consent = .rejected
            state.warning = reason
            state.media = .none
            state.firstFrame = false
            release(&state, &effects)
        case .consentCancelled:
            state.consent = .cancelled
            state.media = .none
            state.firstFrame = false
            release(&state, &effects)
        case .rtcConnected:
            if !state.firstFrame { state.media = .waitingForFirstFrame }
        case .firstFrame(let target, let generation, let geometry):
            guard current(target, state), state.permissions["video.view"] == .granted,
                  state.consent == .approved, generation >= state.decoderGeneration,
                  geometry >= state.geometryEpoch else { effects.append(.ignoredStaleEvent); return }
            state.decoderGeneration = generation
            state.firstFrame = true
            state.media = .live
            state.geometryEpoch = geometry
            state.geometryReady = true
        case .decoderWaiting(let target, let generation):
            guard current(target, state), generation >= state.decoderGeneration else { return }
            state.decoderGeneration = generation
            state.firstFrame = false
            state.media = .recovering
            effects.append(.requestKeyframe)
        case .geometry(let target, let epoch):
            guard current(target, state), epoch > state.geometryEpoch else { return }
            state.geometryEpoch = epoch
            state.geometryReady = false
            effects.append(.requestGeometry)
        case .lease(let lease):
            guard current(lease.target, state), state.foreground,
                  state.permissions["input.control"] == .granted, lease.expiresAt > state.now else { return }
            state.lease = lease
        case .leaseRevoked(let id):
            if state.lease?.id == id { release(&state, &effects) }
        case .targetClosed(let target):
            state.closedTargets[target.id] = max(state.closedTargets[target.id] ?? UInt64(0), target.generation)
            state.catalog.removeAll { $0.id == target.id && $0.generation <= target.generation }
            if let selected = state.target, selected.id == target.id && selected.generation <= target.generation {
                state.media = .closed
                state.firstFrame = false
                state.permissions = [:]
                state.pending = [:]
                state.lastConfirmedAction = nil
                release(&state, &effects)
            }
        case .actionResult(let id, let target, let revision, let success):
            guard current(target, state), let pending = state.pending[id], pending.target == target,
                  pending.revision == revision, state.capabilityRevision == revision, state.capabilitiesFresh else {
                effects.append(.ignoredStaleEvent)
                return
            }
            state.pending.removeValue(forKey: id)
            if success { state.lastConfirmedAction = pending.action }
        case .disconnected:
            state.connection = nil
            state.catalog = []
            state.catalogFresh = false
            state.capabilitiesFresh = false
            state.permissions = [:]
            state.pending = [:]
            state.lastConfirmedAction = nil
            state.media = .disconnected
            state.audioPlaying = false
            release(&state, &effects)
        case .sessionFailed(let target):
            guard current(target, state) else { return }
            state.managedSessionFailed = true
            state.media = .closed
            state.firstFrame = false
            state.permissions = [:]
            state.warning = "Managed app failed. Reopen or choose another source."
            release(&state, &effects)
        case .audioPlaying(let playing):
            state.audioPlaying = playing && state.permissions["audio.listen"] == .granted
        }
    }

    private static func route(_ intent: ClientIntent, _ state: inout ClientAppState, _ effects: inout [ClientEffect]) {
        switch intent {
        case .releaseControl: release(&state, &effects)
        case .muteAudio: state.audioPlaying = false
        case .inputMode(let mode):
            if mode == "direct_touch" && !state.features.contains("input.touch") {
                state.inputMode = "pointer"
                state.warning = "Direct touch unavailable. Use pointer mode."
            } else { state.inputMode = mode }
        case .openPicker(let kind):
            guard state.authenticated && state.capabilitiesFresh && state.features.contains("capture.picker") else { return }
            release(&state, &effects)
            state.requestedSourceKind = kind
            state.consent = .waiting
            state.firstFrame = false
            state.media = .none
            effects.append(.openPicker(kind))
        case .invoke(let action, let request):
            guard let target = state.target, let revision = state.capabilityRevision,
                  state.pending[request] == nil, state.availableActions.contains(where: { $0.id == action }) else { return }
            state.pending[request] = ClientPendingAction(target: target, revision: revision, action: action)
            effects.append(.action(request, action, target, revision))
        case .keyDown(let usage):
            guard state.canSendInput else { return }
            if !state.heldKeys.contains(usage) { state.heldKeys.append(usage) }
            effects.append(.keyDown(usage))
        case .absolutePointer(let x, let y, let epoch, let inside):
            guard epoch == state.geometryEpoch && state.geometryReady else { effects.append(.requestGeometry); return }
            guard state.canSendInput && inside && x.isFinite && y.isFinite && x >= 0 && x <= 1 && y >= 0 && y <= 1 else { return }
            effects.append(.absolutePointer(x, y, epoch))
        case .clipboard(let read):
            guard read ? state.canReadClipboard : state.canWriteClipboard else { return }
            effects.append(.clipboard(read))
        }
    }
}
