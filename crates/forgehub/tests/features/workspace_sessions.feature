Feature: Workspace sessions

  Scenario: User sees only sessions in active workspace
    Given a hub with workspace root "ws-a" at "/repos/a"
    And a hub with workspace root "ws-b" at "/repos/b"
    And sessions exist at repo roots "/repos/a/project-a" and "/repos/b/project-b"
    When I list sessions for workspace "ws-a"
    Then the session list contains session for repo root "/repos/a/project-a"
    And the session list excludes session for repo root "/repos/b/project-b"
