Feature: Worktree session start
  Scenario: Start session with worktree and branch
    Given an edge server accepts session starts
    When I register the edge with the hub
    And I start a session in repo "/repos/alpha" with worktree "true" and branch "feat-x"
    Then the edge receives a start request with worktree "true" and branch "feat-x"
