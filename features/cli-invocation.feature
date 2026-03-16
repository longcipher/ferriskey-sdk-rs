Feature: FerrisKey CLI invocation
  As an operator
  I want one CLI subcommand per documented FerrisKey API operation
  So that I can exercise the full API contract from the terminal

  Scenario: CLI command groups mirror API tags
    Given the FerrisKey CLI is built from the normalized contract registry
    When I ask the CLI for its top-level help output
    Then I should see one command group for each documented API tag

  Scenario: CLI flags and JSON body arguments invoke the same contract as the SDK
    Given Prism is serving the normalized FerrisKey contract
    And the CLI is configured with a base URL and optional bearer token
    When I invoke a documented operation through CLI subcommands and arguments
    Then the CLI should call the same contract-defined operation as the SDK
    And the CLI should print the documented response in a structured format