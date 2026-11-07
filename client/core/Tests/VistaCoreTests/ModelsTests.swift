import XCTest
@testable import VistaCore

final class ModelsTests: XCTestCase {
    func testCatalogHasDistinctStableIDs() {
        let ids = DemoCatalog.applications.map(\.id)
        XCTAssertEqual(Set(ids).count, ids.count)
        XCTAssertTrue(DemoCatalog.applications.contains { $0.source == .existingWindow })
        XCTAssertTrue(DemoCatalog.applications.contains { $0.source == .independentSession })
    }

    func testApplicationRoundTripsAsJSON() throws {
        let original = DemoCatalog.applications[0]
        let data = try JSONEncoder().encode(original)
        let decoded = try JSONDecoder().decode(RemoteApplication.self, from: data)
        XCTAssertEqual(decoded, original)
    }

    func testHostEndpointValidatesWithoutAttemptingAConnection() {
        XCTAssertEqual(HostEndpoint(hostname: "workstation.local")?.displayAddress,
                       "workstation.local:7443")
        XCTAssertEqual(HostEndpoint(hostname: "2001:db8::1", port: 9000)?.displayAddress,
                       "[2001:db8::1]:9000")
        XCTAssertNil(HostEndpoint(hostname: ""))
        XCTAssertNil(HostEndpoint(hostname: "https://example.com"))
        XCTAssertNil(HostEndpoint(hostname: "example.com", port: 0))
        XCTAssertNil(HostEndpoint(hostname: "example.com", port: 65536))
    }

    func testEndpointRoundTripsAsJSON() throws {
        let endpoint = try XCTUnwrap(HostEndpoint(hostname: "machine.lan", port: 7443))
        let encoded = try JSONEncoder().encode(endpoint)
        XCTAssertEqual(try JSONDecoder().decode(HostEndpoint.self, from: encoded), endpoint)
    }
}
