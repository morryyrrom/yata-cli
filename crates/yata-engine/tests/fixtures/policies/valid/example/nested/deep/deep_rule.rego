package yata.rules.test_policies.example.nested.deep.deep_rule

import rego.v1

metadata := {
	"title": "Deeply nested rule",
	"severity": "info",
	"description": "Checks that rules can be nested in several directories.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains finding if {
	some r in input.aws.example.widget
	r.tags.flag == "bad"
	finding := {"resource_id": r.id, "message": sprintf("Widget %s is flagged", [r.id])}
}
