Feature: Session input proxy
  Scenario: User sends session input through the hub
    Given an edge server accepts session inputs for session "S-EDGE"
    When I register the edge with the hub
    And I send session input "status" for session "S-EDGE"
    Then the edge receives session input "status"
