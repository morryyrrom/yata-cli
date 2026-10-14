package yata.rules.test_policies.fail_missing.partial_set

import rego.v1

metadata := {
	"title": "fail as a partial set",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}

# The old form of the rule: not supported any more.
fail contains finding if {
	some r in input.aws.example.widget
	finding := {"resource_id": r.id, "message": "m"}
}
