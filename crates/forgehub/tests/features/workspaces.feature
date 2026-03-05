Feature: Workspaces

  Scenario: Default workspace is seeded
    Given a hub with no configured workspaces
    When I list workspaces
    Then the workspace list contains "default"

  Scenario: User can switch workspaces
    Given a hub with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a hub with workspace "ws-b" named "Beta" and repo "beta" labeled "Beta" rooted at "/repos/b"
    When I select workspace "ws-b"
    Then the active workspace is "ws-b"

  Scenario: Configured workspace includes repos
    Given a hub with workspace "ws-checkout" named "Checkout" and repo "forgemux" labeled "Forgemux" rooted at "/repos/forgemux"
    When I get workspace "ws-checkout"
    Then the workspace has repo "forgemux" labeled "Forgemux"
