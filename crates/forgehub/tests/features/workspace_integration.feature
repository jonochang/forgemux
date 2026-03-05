Feature: Workspace integration

  Scenario: User lists workspaces and sessions through HTTP
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a hub server with workspace "ws-b" named "Beta" and repo "beta" labeled "Beta" rooted at "/repos/b"
    And an edge server provides sessions for repo roots "/repos/a/project-a" and "/repos/b/project-b"
    When I register the edge with the hub
    And I request workspaces via HTTP
    Then the HTTP workspace list contains "ws-a" and "ws-b"
    When I request sessions for workspace "ws-a" via HTTP
    Then the HTTP session list contains session for repo root "/repos/a/project-a"
    And the HTTP session list excludes session for repo root "/repos/b/project-b"
