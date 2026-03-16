Feature: Prism-backed contract verification
  As a maintainer
  I want the SDK and CLI validated against a Prism mock server
  So that contract drift is detected before release

  Scenario: Prism sweep validates every documented operation
    Given Prism is serving the normalized FerrisKey contract
    When I run the SDK contract sweep
    Then every documented operation should be exercised exactly once
    And no documented operation should remain uncovered