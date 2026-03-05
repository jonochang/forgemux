Feature: Session creation

  Scenario: User starts a session with a name and model
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And an edge server accepts session starts
    When I register the edge with the hub
    And I start a session named "Checkout fixes" with model "haiku"
    Then the edge receives a start request with model "haiku" and name "Checkout fixes"
