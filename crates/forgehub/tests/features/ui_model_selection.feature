Feature: Dashboard model selection
  Scenario Outline: Selecting a model sets the start payload
    Given a hub dashboard is running
    And an edge server advertises models and accepts session starts
    When I register the edge with the hub
    And I open the dashboard
    And I select agent "<agent>"
    And I select model "<model>"
    And I enter repo "<repo>"
    And I start the session
    Then the edge receives a start request with agent "<agent>" and model "<model>"

    Examples:
      | agent  | model          | repo         |
      | claude | haiku          | /repos/alpha |
      | codex  | gpt-5.3-codex  | /repos/alpha |
