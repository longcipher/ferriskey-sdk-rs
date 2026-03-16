Feature: FerrisKey SDK contract coverage
  As an SDK integrator
  I want every documented FerrisKey API operation exposed through Rust APIs
  So that I can call the service without hand-writing HTTP requests

  Scenario: SDK exposes every documented operation
    Given the FerrisKey contract has been normalized from docs/openai.json
    When I inspect the generated SDK operation registry
    Then every documented operationId should have a callable SDK entrypoint
    And the SDK should group those operations by the documented API tags

  Scenario: Secured operations apply bearer auth and decode structured responses
    Given Prism is serving the normalized FerrisKey contract
    And the SDK is configured with a bearer token
    When I invoke a secured SDK operation
    Then the request should include the bearer authorization header
    And the response should decode into the documented typed result