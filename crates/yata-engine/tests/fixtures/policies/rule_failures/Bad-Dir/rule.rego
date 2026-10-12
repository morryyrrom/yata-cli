package yata.rules.test_policies.bad_dir.rule

import rego.v1

metadata := {
	"title": "Test rule bad_dir.rule",
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
