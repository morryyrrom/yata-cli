package yata.rules.test_policies.fail_element.unknown_id_region

import rego.v1

metadata := {
	"title": "Test rule fail_element.unknown_id_region",
	"severity": "medium",
	"description": "Test rule used by the engine tests.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains finding if {
	some r in input.aws.example.widget
	r.tags.flag == "bad"
	finding := {"resource_id": r.id, "resource_region": "eu-west-1", "message": "m"}
}
