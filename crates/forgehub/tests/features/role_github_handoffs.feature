Feature: Role handoffs linked to GitHub issues

  Scenario: Create handoff from valid GitHub issue
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a github server has issue "acme/forge" number 42
    When I create handoff from "implementer" to "reviewer_tester" for issue "acme/forge" number 42
    Then the handoff request succeeds
    When I list handoffs for role "reviewer_tester" and status "queued"
    Then the handoff list has 1 item
    And the latest handoff is for role "reviewer_tester" with status "queued"

  Scenario: Reject handoff for unknown issue
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a github server has no issue "acme/forge" number 404
    When I create handoff from "implementer" to "reviewer_tester" for issue "acme/forge" number 404
    Then the handoff request fails with bad request

  Scenario: Reviewer claim lock allows one claimer
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a github server has issue "acme/forge" number 51
    When I create handoff from "implementer" to "reviewer_tester" for issue "acme/forge" number 51
    Then the handoff request succeeds
    When I claim the handoff as "alice"
    Then the handoff request succeeds
    When I claim the handoff as "bob"
    Then the handoff action fails with conflict

  Scenario: Approve review and promote to SRE queue
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a github server has issue "acme/forge" number 52
    When I create handoff from "implementer" to "reviewer_tester" for issue "acme/forge" number 52
    And I claim the handoff as "alice"
    And I complete the handoff with outcome "approve" as "alice"
    And I promote the handoff
    When I list handoffs for role "sre" and status "queued"
    Then the handoff list has 1 item
    And github has at least 2 handoff comments

  Scenario: Request changes returns work to implementer queue
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a github server has issue "acme/forge" number 53
    When I create handoff from "implementer" to "reviewer_tester" for issue "acme/forge" number 53
    And I claim the handoff as "alice"
    And I complete the handoff with outcome "request_changes" as "alice"
    When I list handoffs for role "implementer" and status "queued"
    Then the handoff list has 1 item

  Scenario: GitHub close webhook marks linked handoff as needs attention
    Given a hub server with workspace "ws-a" named "Alpha" and repo "alpha" labeled "Alpha" rooted at "/repos/a"
    And a github server has issue "acme/forge" number 54
    When I create handoff from "implementer" to "reviewer_tester" for issue "acme/forge" number 54
    And GitHub sends issue "acme/forge" number 54 as closed
    When I list handoffs for role "reviewer_tester" and status "needs_attention"
    Then the handoff list has 1 item
