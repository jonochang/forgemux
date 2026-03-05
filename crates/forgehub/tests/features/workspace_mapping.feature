Feature: Workspace mapping

  Scenario: Repo roots map to the right workspace
    Given a hub with workspace root "ws-a" at "/repos/a"
    And a hub with workspace root "ws-b" at "/repos/b"
    When I resolve workspace for repo root "/repos/a/project"
    Then the resolved workspace id is "ws-a"
