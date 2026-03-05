Feature: Decision lifecycle
  Scenario: Reviewer approves a decision and edge is notified
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And an edge server accepts decision responses for session "S-EDGE"
    When I register the edge with the hub
    And I create a decision for session "S-EDGE" in workspace "ws-a"
    And I list decisions for workspace "ws-a"
    Then the decision list contains the new decision
    When I approve the decision as "Jono"
    Then the edge receives decision response "approve" by "Jono"
