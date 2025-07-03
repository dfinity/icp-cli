// Provides a test server for integration tests
use httptest::{Expectation, Server, matchers::*, responders::*};

// Spawns a test server that expects a single request and responds with a 200 status code and the given body
pub fn spawn_test_server(method: &str, path: &str, body: &[u8]) -> httptest::Server {
    // Run the server
    let server = Server::run();

    // Set up the expectation
    server.expect(
        Expectation::matching(request::method_path(method.to_owned(), path.to_owned()))
            .times(1)
            .respond_with(status_code(200).body(body.to_owned())),
    );

    // Return the server instance
    server
}
