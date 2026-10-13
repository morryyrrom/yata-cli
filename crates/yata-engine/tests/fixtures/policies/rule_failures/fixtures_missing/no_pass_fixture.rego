package yata.rules.test_policies.fixtures_missing.no_pass_fixture

import rego.v1

metadata := {
	"title": "Test rule fixtures_missing.no_pass_fixture",
	"severity": "medium",
	"description": "Test rule used by the engine tests.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains finding if {
	some r in input.aws.example.widget
	r.tags.flag == "bad"
	finding := {"resource_id": r.id, "message": sprintf("Resource %s is flagged", [r.id])}
}
